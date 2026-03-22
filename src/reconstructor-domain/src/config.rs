// =============================================================================
// PipelineConfig — Topología de configuración base
//
// Propósito: Define el contrato tipado y determinista que mapea las tolerancias y
//            heurísticas del usuario (desde TOML o UI) hacia el núcleo del dominio
//            sin exponer dependencias de serialización a la lógica pura.
// =============================================================================

use serde::{Deserialize, Serialize};

/// Estructura de inyección de parámetros para el Orquestador y las Fábricas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    pub general: GeneralConfig,
    pub input: InputConfig,
    pub orientation: OrientationConfig,
    pub layout: LayoutConfig,
    pub ocr: OcrConfig,
    pub table: TableConfig,
    pub output: OutputConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    pub models_dir: String,
    pub log_level: String,
    
    /// Regula la saturación de los procesadores lógicos durante operaciones
    /// CPU-bound (Rayon). Valor en 0 activa la auto-topología según la arquitectura.
    #[serde(default)]
    pub num_threads: usize,
    
    /// Barrera de sobrecarga de RAM. Delimita cuántas páginas existen en memoria
    /// rasterizada simultáneamente, evitando OOM en PDFs densos.
    #[serde(default)]
    pub max_concurrent_pages: usize,
    
    /// Switch de provisión de backend tensor:
    /// Define si `ort` debe instanciar ExecutionProviders (CUDA/DirectML) antes de
    /// hacer fallback seguro al procesador.
    #[serde(default)]
    pub use_gpu: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputConfig {
    pub rasterization_dpi: u32,
    pub supported_formats: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrientationConfig {
    pub page_threshold: f32,
    pub textline_threshold: f32,
    pub textline_batch_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutConfig {
    pub detection_threshold: f32,
    pub nms_threshold: f32,
    pub input_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrConfig {
    pub confidence_threshold: f32,
    pub max_retries: u32,
    pub max_unrecognizable_ratio: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableConfig {
    pub structure_threshold: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    pub formats: Vec<String>,
    pub default_font: String,
    pub default_font_size: f32,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig {
                models_dir: "models".into(),
                log_level: "info".into(),
                num_threads: 0,
                max_concurrent_pages: 0,
                use_gpu: false,
            },
            input: InputConfig {
                rasterization_dpi: 300,
                supported_formats: vec![
                    "pdf".into(), "png".into(), "jpg".into(),
                    "jpeg".into(), "tiff".into(), "webp".into(),
                ],
            },
            orientation: OrientationConfig {
                page_threshold: 0.85,
                textline_threshold: 0.90,
                textline_batch_size: 32,
            },
            layout: LayoutConfig {
                detection_threshold: 0.50,
                nms_threshold: 0.45,
                input_size: 1024,
            },
            ocr: OcrConfig {
                confidence_threshold: 0.60,
                max_retries: 1,
                max_unrecognizable_ratio: 0.30,
            },
            table: TableConfig {
                structure_threshold: 0.50,
            },
            output: OutputConfig {
                formats: vec!["pdf".into(), "txt".into(), "json".into()],
                default_font: "Liberation Sans".into(),
                default_font_size: 11.0,
            },
        }
    }
}
