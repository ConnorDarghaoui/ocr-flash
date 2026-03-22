// =============================================================================
// reconstructor-infra — Anillo 4: Infrastructure Adapters
//
// Propósito: Implementaciones concretas de los traits del dominio.
//            Es el único lugar donde viven ONNX Runtime, image, printpdf,
//            reqwest, etc. El dominio no conoce estas dependencias.
//
// Oleada A (sin ONNX): error, fallback, output, composer, input, models
// Oleada B (con ONNX): orientation, layout, ocr, table
// =============================================================================

pub mod composer;
pub mod error;
pub mod fallback;
pub mod input;
pub mod layout;
pub mod models;
pub mod ocr;
pub mod ort_session;
pub mod orientation;
pub mod output;
pub mod table;

// --- Re-exports de conveniencia ---

pub use composer::PrintPdfComposer;
pub use error::InfraError;
pub use fallback::RasterFallbackResolver;
pub use input::{ImageInputReader, PdfiumPageReader};
pub use layout::DocLayoutYoloDetector;
pub use models::{HuggingFaceModelProvider, ModelManifest};
pub use ocr::PaddleOcrResolver;
pub use orientation::{OnnxOrientationCorrector, OnnxTextlineOrientationCorrector};
pub use output::{JsonOutputGenerator, PdfOutputGenerator, TxtOutputGenerator};
pub use table::SlaNetTableResolver;
