// =============================================================================
// pipeline_bench — Benchmarks de rendimiento del pipeline (F4.3)
//
// Mide el throughput de las etapas sin ONNX (fallback-only):
//   - RasterFallbackResolver: resolución de un bloque raster
//   - JsonOutputGenerator + TxtOutputGenerator: serialización
//   - PrintPdfComposer: composición de una página PDF
//   - Pipeline E2E fallback: 1-10 páginas completas
//   - EvalMetrics: CER y Layout IoU
//
// Ejecutar: cargo bench -p reconstructor-infra
// Reporte HTML: target/criterion/report/index.html
// =============================================================================

use std::io::Cursor;
use std::sync::mpsc;

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use image::{DynamicImage, ImageFormat};

use reconstructor_app::PipelineOrchestrator;
use reconstructor_domain::{
    BoundingBox, BlockType, DomainError, LayoutDetector, OrientationCorrector, OrientationResult,
    PageImage, PipelineConfig, Region, ResolverFactory, cer, layout_iou,
};
use reconstructor_infra::{
    PrintPdfComposer, RasterFallbackResolver, TxtOutputGenerator,
};

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn crear_png_sintetico(ancho: u32, alto: u32) -> Vec<u8> {
    let img = DynamicImage::ImageRgb8(image::RgbImage::new(ancho, alto));
    let mut bytes = Vec::new();
    img.write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png).unwrap();
    bytes
}

fn crear_region(ancho: f32, alto: f32) -> Region {
    Region {
        id: "blk_1_0".to_string(),
        tipo_bloque: BlockType::Text,
        bbox: BoundingBox { x: 0.0, y: 0.0, width: ancho, height: alto },
        confianza_deteccion: 0.95,
    }
}

fn crear_paginas(n: u32, ancho: u32, alto: u32) -> Vec<PageImage> {
    let datos = crear_png_sintetico(ancho, alto);
    (1..=n)
        .map(|i| PageImage { datos: datos.clone(), ancho, alto, numero_pagina: i })
        .collect()
}

// ─── Stubs mínimos para E2E bench ────────────────────────────────────────────

struct NoopOrientation;
impl OrientationCorrector for NoopOrientation {
    fn corregir_pagina(
        &self,
        b: &[u8],
        _: u32,
        _: u32,
    ) -> Result<(Vec<u8>, OrientationResult), DomainError> {
        Ok((b.to_vec(), OrientationResult { angulo_grados: 0.0, confianza: 1.0, incierto: false }))
    }
}

struct FullPageLayout;
impl LayoutDetector for FullPageLayout {
    fn detectar(
        &self,
        _: &[u8],
        ancho: u32,
        alto: u32,
        num_pagina: u32,
    ) -> Result<Vec<Region>, DomainError> {
        Ok(vec![Region {
            id: format!("blk_{num_pagina}_0"),
            tipo_bloque: BlockType::Text,
            bbox: BoundingBox { x: 0.0, y: 0.0, width: ancho as f32, height: alto as f32 },
            confianza_deteccion: 1.0,
        }])
    }
}

fn crear_orchestrator(config: &PipelineConfig) -> (PipelineOrchestrator, mpsc::Receiver<reconstructor_app::PipelineEvent>) {
    PipelineOrchestrator::new(
        Box::new(NoopOrientation),
        Box::new(FullPageLayout),
        ResolverFactory::new(vec![Box::new(RasterFallbackResolver)]),
        Box::new(PrintPdfComposer),
        vec![Box::new(TxtOutputGenerator)],
        config.clone(),
    )
}

// ─── Benchmarks ──────────────────────────────────────────────────────────────

fn bench_raster_fallback(c: &mut Criterion) {
    let mut group = c.benchmark_group("RasterFallback");

    for size in [32u32, 128, 512] {
        let png = crear_png_sintetico(size, size);
        let region = crear_region(size as f32, size as f32);
        let resolver = RasterFallbackResolver;

        group.bench_with_input(
            BenchmarkId::new("resolver_bloque", format!("{size}x{size}px")),
            &size,
            |b, _| {
                b.iter(|| {
                    use reconstructor_domain::traits::BlockResolver;
                    resolver.resolver(black_box(&region), black_box(&png)).unwrap()
                });
            },
        );
    }

    group.finish();
}

fn bench_txt_output(c: &mut Criterion) {
    // Benchmark serialización TXT usando el pipeline completo en fallback
    let mut group = c.benchmark_group("TxtOutput");
    let config = PipelineConfig::default();
    let dir = std::env::temp_dir().join("bench_txt");
    std::fs::create_dir_all(&dir).unwrap();

    for n_pags in [1u32, 5, 10] {
        let paginas = crear_paginas(n_pags, 200, 100);
        let ruta = dir.join(format!("bench_{n_pags}p")).to_string_lossy().to_string();

        group.bench_with_input(
            BenchmarkId::new("pipeline_fallback", format!("{n_pags}pags")),
            &n_pags,
            |b, _| {
                b.iter(|| {
                    let (orch, _rx) = crear_orchestrator(&config);
                    orch.procesar(black_box(paginas.clone()), black_box(&ruta)).unwrap()
                });
            },
        );
    }

    group.finish();
}

fn bench_pipeline_e2e(c: &mut Criterion) {
    let config = PipelineConfig::default();
    let dir = std::env::temp_dir().join("bench_e2e");
    std::fs::create_dir_all(&dir).unwrap();
    let ruta = dir.join("out").to_string_lossy().to_string();

    // A4 a 72 DPI (~595×842 px) para benchmark rápido
    let paginas_1 = crear_paginas(1, 595, 842);
    let paginas_5 = crear_paginas(5, 595, 842);

    c.bench_function("pipeline_e2e_1pag_A4_72dpi", |b| {
        b.iter(|| {
            let (orch, _rx) = crear_orchestrator(&config);
            orch.procesar(black_box(paginas_1.clone()), black_box(&ruta)).unwrap()
        });
    });

    c.bench_function("pipeline_e2e_5pags_A4_72dpi", |b| {
        b.iter(|| {
            let (orch, _rx) = crear_orchestrator(&config);
            orch.procesar(black_box(paginas_5.clone()), black_box(&ruta)).unwrap()
        });
    });
}

fn bench_eval_metrics(c: &mut Criterion) {
    let mut group = c.benchmark_group("EvalMetrics");

    // CER con diferentes longitudes de texto
    let textos: &[(&str, &str, &str)] = &[
        (
            "corto",
            "Hola mundo",
            "Hola mund0",
        ),
        (
            "parrafo",
            "El sistema de reconocimiento óptico de caracteres procesa documentos complejos con alta precisión.",
            "El sistema de reconocimineto óptico de caracters procesa documentos complejos con alta precison.",
        ),
        (
            "pagina",
            &"Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(20),
            &"Lorem lpsum dolor sit amet, consectetur adipiscing elt. ".repeat(20),
        ),
    ];

    for (name, gt, pred) in textos {
        group.bench_with_input(
            BenchmarkId::new("cer", name),
            &(*gt, *pred),
            |b, (gt, pred)| {
                b.iter(|| cer(black_box(pred), black_box(gt)));
            },
        );
    }

    // Layout IoU con N cajas
    for n_cajas in [5usize, 20, 50] {
        let bboxes: Vec<BoundingBox> = (0..n_cajas)
            .map(|i| BoundingBox {
                x: (i as f32) * 60.0,
                y: 0.0,
                width: 50.0,
                height: 50.0,
            })
            .collect();

        group.bench_with_input(
            BenchmarkId::new("layout_iou", format!("{n_cajas}cajas")),
            &bboxes,
            |b, bboxes| {
                b.iter(|| layout_iou(black_box(bboxes), black_box(bboxes)));
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_raster_fallback,
    bench_txt_output,
    bench_pipeline_e2e,
    bench_eval_metrics,
);
criterion_main!(benches);
