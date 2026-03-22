// =============================================================================
// reconstructor-app — Anillo 3: Application Services
//
// Proposito: Orquesta el pipeline OCR completo usando los traits del dominio
//            como interfaces. No depende de ONNX, printpdf ni ninguna infra concreta.
//            Los adapters se inyectan desde el Composition Root (reconstructor-ui).
//
// Modulos:
//   error       — AppError: errores de la capa de orquestacion
//   events      — PipelineEvent: canal Observer hacia la UI
//   orchestrator — PipelineOrchestrator: coordinador del pipeline
//   job         — JobManager: gestion del ciclo de vida del job
// =============================================================================

pub mod error;
pub mod events;
pub mod job;
pub mod orchestrator;
pub mod threading;

pub use error::AppError;
pub use events::PipelineEvent;
pub use job::JobManager;
pub use orchestrator::PipelineOrchestrator;
pub use threading::{inicializar_thread_pool, num_threads_efectivos};
