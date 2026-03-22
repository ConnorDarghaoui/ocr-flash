// =============================================================================
// fsm — Autómata Jerarquico de 3 Niveles (ADR-009, seccion 5.2.1 del SRS)
//
// Nivel 1: DocumentFSM — ciclo de vida completo del procesamiento
// Nivel 2: PageFSM     — procesamiento de una pagina individual
// Nivel 3: BlockFSM    — resolucion de una region individual
//
// Los 3 FSMs comparten el patron: estados como enums, eventos como enums,
// funciones transition() puras sin side effects, testeables con match exhaustivo.
// =============================================================================

pub mod block;
pub mod document;
pub mod history;
pub mod page;
pub mod transition;

// Re-exports de conveniencia
pub use block::{BlockEvent, BlockState};
pub use document::{DocumentEvent, DocumentState, DownloadProgress, OutputStage};
pub use history::StateTransition;
pub use page::{PageEvent, PageState};
pub use transition::{
    transition_block, transition_document, transition_page,
};
