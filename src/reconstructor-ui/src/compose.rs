// =============================================================================
// compose — Composition Root: instancia adapters e inyecta en el orchestrator
//
// Propósito: Único lugar donde se conocen todos los tipos concretos de infra.
//            Decide qué adapters usar según disponibilidad de modelos y flags CLI.
//            Incluye stubs de degradación graceful cuando los modelos no existen.
// =============================================================================

use std::sync::mpsc::Receiver;

use anyhow::Context;
use reconstructor_app::{PipelineEvent, PipelineOrchestrator};
use reconstructor_domain::{
    BoundingBox, BlockResolver, BlockType, DomainError, LayoutDetector, ModelEntry,
    OrientationCorrector, OrientationResult, OutputGenerator, PageComposer, PipelineConfig,
    Region, ResolverFactory,
};
use reconstructor_infra::{
    DocLayoutYoloDetector, JsonOutputGenerator, OnnxOrientationCorrector,
    OnnxTextlineOrientationCorrector, PaddleOcrResolver, PdfOutputGenerator, PrintPdfComposer,
    RasterFallbackResolver, SlaNetTableResolver, TxtOutputGenerator,
};

/// Resultado de la construcción del sistema: orchestrator listo + canal de eventos.
pub struct CompositionRoot {
    pub orchestrator: PipelineOrchestrator,
    pub event_rx: Receiver<PipelineEvent>,
}

/// Busca el `path_local` de un modelo por su ID en el manifest.
fn path_modelo(entries: &[ModelEntry], id: &str) -> Option<String> {
    entries.iter().find(|e| e.id == id).map(|e| e.path_local.clone())
}

/// Devuelve true si el archivo local del modelo existe en disco.
fn modelo_existe(entries: &[ModelEntry], id: &str) -> bool {
    path_modelo(entries, id)
        .map(|p| std::path::Path::new(&p).exists())
        .unwrap_or(false)
}

/// Construye todos los adapters e inyecta en `PipelineOrchestrator`.
///
/// - `entries`: entradas del model_manifest (proveen los `path_local` de cada modelo)
/// - `formats`: formatos de salida a generar (ya resueltos: CLI override o config default)
/// - `fallback_only`: si true, omite todos los adapters ONNX aunque los modelos existan
pub fn construir(
    config: &PipelineConfig,
    entries: &[ModelEntry],
    formats: &[String],
    fallback_only: bool,
) -> anyhow::Result<CompositionRoot> {
    let use_gpu = config.general.use_gpu;
    if use_gpu {
        tracing::info!("GPU acceleration habilitada — intentando CUDA/DirectML/CoreML");
    }

    // ── OrientationCorrector ─────────────────────────────────────────────────
    let orientation: Box<dyn OrientationCorrector> =
        if !fallback_only && modelo_existe(entries, "orientation_page") {
            let path = path_modelo(entries, "orientation_page").unwrap();
            tracing::info!("Cargando modelo de orientación: {path}");
            Box::new(
                OnnxOrientationCorrector::new_with_gpu(&path, use_gpu)
                    .with_context(|| format!("No se pudo cargar modelo de orientación: {path}"))?,
            )
        } else {
            tracing::warn!("Usando corrector de orientación noop (modelo ausente o fallback_only)");
            Box::new(NoopOrientationCorrector)
        };

    // ── LayoutDetector ───────────────────────────────────────────────────────
    let layout: Box<dyn LayoutDetector> =
        if !fallback_only && modelo_existe(entries, "layout") {
            let path = path_modelo(entries, "layout").unwrap();
            tracing::info!("Cargando modelo de layout: {path}");
            Box::new(
                DocLayoutYoloDetector::new_with_gpu(&path, use_gpu)
                    .with_context(|| format!("No se pudo cargar modelo de layout: {path}"))?,
            )
        } else {
            tracing::warn!("Usando detector de layout full-page (modelo ausente o fallback_only)");
            Box::new(FullPageLayoutDetector)
        };

    // ── ResolverFactory ──────────────────────────────────────────────────────
    let resolvers = {
        let usar_paddle = !fallback_only
            && modelo_existe(entries, "ocr_det")
            && modelo_existe(entries, "ocr_rec")
            && modelo_existe(entries, "ocr_dict");

        if usar_paddle {
            let det = path_modelo(entries, "ocr_det").unwrap();
            let rec = path_modelo(entries, "ocr_rec").unwrap();
            let dict = path_modelo(entries, "ocr_dict").unwrap();
            tracing::info!("Cargando PaddleOCR: det={det}, rec={rec}, dict={dict}");
            let paddle = PaddleOcrResolver::new_with_gpu(&det, &rec, &dict, use_gpu)
                .context("No se pudo cargar PaddleOcrResolver")?;

            // Textline orientation corrector (Nivel 2) — opcional
            let paddle = if !fallback_only && modelo_existe(entries, "orientation_textline") {
                let tl_path = path_modelo(entries, "orientation_textline").unwrap();
                match OnnxTextlineOrientationCorrector::new_with_gpu(&tl_path, use_gpu) {
                    Ok(tc) => {
                        tracing::info!("Textline orientation corrector cargado: {tl_path}");
                        paddle.with_textline_corrector(Box::new(tc))
                    }
                    Err(e) => {
                        tracing::warn!("No se pudo cargar textline corrector: {e}");
                        paddle
                    }
                }
            } else {
                paddle
            };

            let usar_slanet = !fallback_only && modelo_existe(entries, "table");
            let mut resolver_list: Vec<Box<dyn BlockResolver>> = vec![
                Box::new(paddle) as Box<dyn BlockResolver>,
            ];
            if usar_slanet {
                let table_path = path_modelo(entries, "table").unwrap();
                match SlaNetTableResolver::new_with_gpu(&table_path, use_gpu) {
                    Ok(r) => {
                        tracing::info!("SLANet+ cargado: {table_path}");
                        resolver_list.push(Box::new(r) as Box<dyn BlockResolver>);
                    }
                    Err(e) => tracing::warn!("No se pudo cargar SLANet+: {e}"),
                }
            }
            resolver_list.push(Box::new(RasterFallbackResolver) as Box<dyn BlockResolver>);
            ResolverFactory::new(resolver_list)
        } else {
            tracing::warn!("Usando solo RasterFallbackResolver (modelos OCR ausentes o fallback_only)");
            ResolverFactory::new(vec![Box::new(RasterFallbackResolver) as Box<dyn BlockResolver>])
        }
    };

    // ── PageComposer ─────────────────────────────────────────────────────────
    let composer: Box<dyn PageComposer> = Box::new(PrintPdfComposer);

    // ── OutputGenerators ─────────────────────────────────────────────────────
    let generators: Vec<Box<dyn OutputGenerator>> = formats
        .iter()
        .filter_map(|f| match f.as_str() {
            "pdf" => Some(Box::new(PdfOutputGenerator) as Box<dyn OutputGenerator>),
            "txt" => Some(Box::new(TxtOutputGenerator) as Box<dyn OutputGenerator>),
            "json" => Some(Box::new(JsonOutputGenerator) as Box<dyn OutputGenerator>),
            other => {
                tracing::warn!("Formato de salida desconocido ignorado: {other}");
                None
            }
        })
        .collect();

    // ── Construir orchestrator ────────────────────────────────────────────────
    let (orchestrator, event_rx) = PipelineOrchestrator::new(
        orientation,
        layout,
        resolvers,
        composer,
        generators,
        config.clone(),
    );

    Ok(CompositionRoot { orchestrator, event_rx })
}

// ─── Stubs de degradación graceful ────────────────────────────────────────────
//
// Usados cuando los modelos no están descargados o se pasa --fallback-only.
// Permiten que el pipeline funcione en modo degradado (raster passthrough).

struct NoopOrientationCorrector;

impl OrientationCorrector for NoopOrientationCorrector {
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

struct FullPageLayoutDetector;

impl LayoutDetector for FullPageLayoutDetector {
    fn detectar(
        &self,
        _imagen_bytes: &[u8],
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
