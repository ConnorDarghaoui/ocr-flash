// =============================================================================
// transition — Fachada de mutadores de estado
//
// Propósito: Evitar la colisión de nombres estáticos (name mangling) al proveer 
//            un punto de acceso unificado para el orquestador a través de alias.
// =============================================================================

pub use super::block::transition as transition_block;
pub use super::document::transition as transition_document;
pub use super::page::transition as transition_page;
