// =============================================================================
// CLI — Definiciones de argumentos con clap
//
// Propósito: Declarar la estructura del CLI usando clap derive.
//            Tres subcomandos: process, download-models, check-models.
// =============================================================================

use clap::{Parser, Subcommand};

/// Reconstructor — Sistema OCR multimodelo para documentos.
#[derive(Parser)]
#[command(name = "reconstructor", about = "OCR multimodelo para documentos")]
pub struct Cli {
    /// Ruta al archivo de configuración TOML.
    #[arg(long, default_value = "config/default.toml")]
    pub config: String,

    /// Ruta al manifest de modelos TOML.
    #[arg(long, default_value = "model_manifest.toml")]
    pub manifest: String,

    #[command(subcommand)]
    pub comando: Comando,
}

#[derive(Subcommand)]
pub enum Comando {
    /// Procesa un archivo de entrada (imagen) por el pipeline OCR.
    Process {
        /// Archivo de entrada (PNG, JPEG, TIFF, WEBP).
        input: String,

        /// Directorio de salida donde se escriben los archivos generados.
        output_dir: String,

        /// Formatos de salida separados por coma: pdf,txt,json.
        /// Si se omite, usa los formatos definidos en el config.
        #[arg(long)]
        formats: Option<String>,

        /// Omitir adapters ONNX; usar solo RasterFallbackResolver.
        /// Útil si los modelos no están descargados.
        #[arg(long)]
        fallback_only: bool,
    },

    /// Descarga modelos ONNX desde HuggingFace según el manifest.
    DownloadModels {
        /// Si se especifica, descarga solo el modelo con este ID.
        #[arg(long)]
        only: Option<String>,
    },

    /// Verifica la presencia e integridad de los modelos instalados.
    CheckModels,

    /// Evalúa el pipeline sobre un directorio de archivos de entrada (F4.2).
    /// Genera un informe HTML con métricas CER, Layout IoU, Fallback Rate y Throughput.
    Evaluate {
        /// Directorio con archivos de entrada (PNG, JPEG, TIFF, WEBP, PDF).
        input_dir: String,

        /// Archivo HTML de salida del informe.
        #[arg(long, default_value = "informe_evaluacion.html")]
        output_html: String,

        /// Usar solo RasterFallbackResolver (sin ONNX).
        #[arg(long)]
        fallback_only: bool,

        /// Título del informe HTML.
        #[arg(long, default_value = "Informe de Evaluación OCR")]
        titulo: String,
    },

    /// Procesa un directorio completo de archivos de entrada en paralelo (modo producción).
    Batch {
        /// Directorio de entrada con PDFs / imágenes.
        input_dir: String,

        /// Directorio raíz de salida. Por cada archivo se crea un subdirectorio.
        output_dir: String,

        /// Número de documentos a procesar en paralelo (0 = número de CPU cores).
        #[arg(long, default_value_t = 0)]
        workers: usize,

        /// Formatos de salida separados por coma: pdf,txt,json.
        #[arg(long)]
        formats: Option<String>,

        /// Omitir adapters ONNX; usar solo RasterFallbackResolver.
        #[arg(long)]
        fallback_only: bool,
    },

    /// Calcula SHA256 de los modelos instalados y actualiza model_manifest.toml.
    UpdateChecksums,
}
