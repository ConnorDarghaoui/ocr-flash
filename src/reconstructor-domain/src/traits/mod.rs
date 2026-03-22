// =============================================================================
// traits — Ports del Dominio (Anillo 1 de la Onion Architecture)
//
// Proposito: Interfaces que el dominio define y que los adapters del anillo 4
//            (reconstructor-infra) implementan. El dominio no conoce las
//            implementaciones concretas — solo los traits.
//
// Cada trait es un Port en el sentido de Hexagonal/Onion Architecture.
// El Composition Root (reconstructor-ui/src/main.rs) instancia los adapters
// e inyecta como Box<dyn Trait> al PipelineOrchestrator.
// =============================================================================

pub mod composer;
pub mod layout;
pub mod model_provider;
pub mod orientation;
pub mod output;
pub mod resolver;

// Re-exports de tipos de datos asociados a los traits
pub use composer::{ComposedPage, PageComposer};
pub use layout::LayoutDetector;
pub use model_provider::{DownloadProgress, ModelEntry, ModelProvider, ModelStatus};
pub use orientation::{OrientationCorrector, OrientationResult, TextlineOrientationCorrector};
pub use output::OutputGenerator;
pub use resolver::BlockResolver;
