// =============================================================================
// Tests de integración end-to-end del pipeline
//
// Estrategia: instanciar adapters reales de infra directamente (sin pasar por
// compose::construir). Siempre en modo fallback_only (sin ONNX).
// =============================================================================

use std::io::Cursor;
use std::sync::mpsc::Receiver;

use image::{DynamicImage, ImageFormat};
use reconstructor_app::{PipelineEvent, PipelineOrchestrator};
use reconstructor_domain::{
    BoundingBox, BlockResolver, BlockType, DomainError, LayoutDetector, OrientationCorrector,
    OrientationResult, OutputGenerator, PageComposer, PageImage, PipelineConfig, Region,
    ResolverFactory,
};
use reconstructor_infra::{
    JsonOutputGenerator, PdfOutputGenerator, PrintPdfComposer, RasterFallbackResolver,
    TxtOutputGenerator,
};

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn crear_png(ancho: u32, alto: u32) -> Vec<u8> {
    let img = DynamicImage::ImageRgb8(image::RgbImage::new(ancho, alto));
    let mut bytes = Vec::new();
    img.write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png).unwrap();
    bytes
}

fn pagina_input(bytes: Vec<u8>, numero: u32) -> PageImage {
    PageImage { datos: bytes, ancho: 100, alto: 100, numero_pagina: numero }
}

fn temp_dir_test(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("reconstructor_test_{}", name));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ─── Stubs locales ───────────────────────────────────────────────────────────

struct NoopOrientation;

impl OrientationCorrector for NoopOrientation {
    fn corregir_pagina(
        &self,
        imagen_bytes: &[u8],
        _ancho: u32,
        _alto: u32,
    ) -> Result<(Vec<u8>, OrientationResult), DomainError> {
        Ok((
            imagen_bytes.to_vec(),
            OrientationResult { angulo_grados: 0.0, confianza: 1.0, incierto: false },
        ))
    }
}

struct FullPageLayout;

impl LayoutDetector for FullPageLayout {
    fn detectar(
        &self,
        _bytes: &[u8],
        ancho: u32,
        alto: u32,
        numero_pagina: u32,
    ) -> Result<Vec<Region>, DomainError> {
        Ok(vec![Region::new(
            &format!("blk_{numero_pagina}_0"),
            BlockType::Text,
            BoundingBox::new(0.0, 0.0, ancho as f32, alto as f32),
            1.0,
        )])
    }
}

// ─── Factory del orchestrator de integración ─────────────────────────────────

fn orchestrator_fallback(
    output_generators: Vec<Box<dyn OutputGenerator>>,
) -> (PipelineOrchestrator, Receiver<PipelineEvent>) {
    PipelineOrchestrator::new(
        Box::new(NoopOrientation),
        Box::new(FullPageLayout),
        ResolverFactory::new(vec![Box::new(RasterFallbackResolver) as Box<dyn BlockResolver>]),
        Box::new(PrintPdfComposer) as Box<dyn PageComposer>,
        output_generators,
        PipelineConfig::default(),
    )
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn pipeline_fallback_una_pagina_genera_documento_valido() {
    let (orq, _rx) = orchestrator_fallback(vec![]);
    let png = crear_png(100, 100);
    let doc = orq.procesar(vec![pagina_input(png, 1)], "/tmp/test_fallback_una").unwrap();

    assert_eq!(doc.paginas.len(), 1);
    assert_eq!(doc.paginas[0].bloques_resueltos.len(), 1);
    assert_eq!(doc.metricas.total_paginas, 1);
    assert_eq!(doc.metricas.bloques_fallback_raster, 1);
    assert_eq!(doc.metricas.bloques_resueltos_texto, 0);
}

#[test]
fn pipeline_fallback_multiples_paginas_orden_correcto() {
    let (orq, _rx) = orchestrator_fallback(vec![]);
    let paginas: Vec<PageImage> =
        (1u32..=3).map(|i| pagina_input(crear_png(100, 100), i)).collect();

    let doc = orq.procesar(paginas, "/tmp/test_fallback_multi").unwrap();

    assert_eq!(doc.paginas.len(), 3);
    assert_eq!(doc.paginas[0].numero_pagina, 1);
    assert_eq!(doc.paginas[1].numero_pagina, 2);
    assert_eq!(doc.paginas[2].numero_pagina, 3);
    assert_eq!(doc.metricas.total_paginas, 3);
    assert_eq!(doc.metricas.bloques_fallback_raster, 3);
}

#[test]
fn pipeline_genera_archivos_pdf_txt_json() {
    let dir = temp_dir_test("genera_archivos");
    let ruta_str = dir.join("test").to_str().unwrap().to_string();

    let generators: Vec<Box<dyn OutputGenerator>> = vec![
        Box::new(PdfOutputGenerator),
        Box::new(TxtOutputGenerator),
        Box::new(JsonOutputGenerator),
    ];
    let (orq, _rx) = orchestrator_fallback(generators);
    orq.procesar(vec![pagina_input(crear_png(100, 100), 1)], &ruta_str).unwrap();

    let pdf = dir.join("test.pdf");
    let txt = dir.join("test.txt");
    let json = dir.join("test.json");

    assert!(pdf.exists(), "test.pdf debe existir");
    assert!(std::fs::metadata(&pdf).unwrap().len() > 0, "test.pdf no debe estar vacío");
    assert!(txt.exists(), "test.txt debe existir");
    assert!(std::fs::metadata(&txt).unwrap().len() > 0, "test.txt no debe estar vacío");
    assert!(json.exists(), "test.json debe existir");
    assert!(std::fs::metadata(&json).unwrap().len() > 0, "test.json no debe estar vacío");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn pipeline_json_tiene_estructura_esperada() {
    let dir = temp_dir_test("json_estructura");
    let ruta_str = dir.join("test").to_str().unwrap().to_string();

    let (orq, _rx) = orchestrator_fallback(vec![Box::new(JsonOutputGenerator)]);
    orq.procesar(vec![pagina_input(crear_png(100, 100), 1)], &ruta_str).unwrap();

    let contenido = std::fs::read_to_string(dir.join("test.json")).unwrap();
    let json: serde_json::Value = serde_json::from_str(&contenido).unwrap();

    assert!(json.get("ruta_origen").is_some(), "JSON debe tener campo ruta_origen");
    assert!(json.get("paginas").is_some(), "JSON debe tener campo paginas");
    assert!(json.get("metricas").is_some(), "JSON debe tener campo metricas");
    assert_eq!(json["metricas"]["total_paginas"], 1);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn pipeline_txt_contiene_separador_de_pagina() {
    let dir = temp_dir_test("txt_separadores");
    let ruta_str = dir.join("test").to_str().unwrap().to_string();

    let (orq, _rx) = orchestrator_fallback(vec![Box::new(TxtOutputGenerator)]);
    let paginas: Vec<PageImage> =
        (1u32..=2).map(|i| pagina_input(crear_png(100, 100), i)).collect();
    orq.procesar(paginas, &ruta_str).unwrap();

    let contenido = std::fs::read_to_string(dir.join("test.txt")).unwrap();
    assert!(contenido.contains("--- Página 1 ---"), "TXT debe contener separador de página 1");
    assert!(contenido.contains("--- Página 2 ---"), "TXT debe contener separador de página 2");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn pipeline_eventos_emitidos_correctamente() {
    let (orq, rx) = orchestrator_fallback(vec![]);
    orq.procesar(vec![pagina_input(crear_png(100, 100), 1)], "/tmp/test_eventos").unwrap();

    let eventos: Vec<_> = rx.try_iter().collect();

    assert!(
        eventos.iter().any(|e| matches!(e, PipelineEvent::PaginaProgreso { .. })),
        "Debe emitirse al menos un PaginaProgreso"
    );
    assert!(
        eventos.iter().any(|e| matches!(e, PipelineEvent::ProcesamientoCompleto { .. })),
        "Debe emitirse ProcesamientoCompleto al final"
    );
    assert!(
        !eventos.iter().any(|e| matches!(e, PipelineEvent::ErrorGlobal(..))),
        "No debe emitirse ErrorGlobal"
    );
}
