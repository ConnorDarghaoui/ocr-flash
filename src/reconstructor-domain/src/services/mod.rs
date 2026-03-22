// =============================================================================
// services — Domain Services (Anillo 2 de la Onion Architecture)
//
// Proposito: Logica de negocio pura que opera sobre las entidades del dominio.
//            No tiene dependencias externas (solo anillo 1 del mismo crate).
//            Testeable sin ningun modelo ONNX cargado.
//
// - ConfidenceEvaluator: decide si la confianza de una inferencia es suficiente.
// - ResolverFactory: selecciona la estrategia de resolucion segun BlockType.
// =============================================================================

pub mod confidence;
pub mod resolver_selection;

pub use confidence::{ConfidenceDecision, ConfidenceEvaluator};
pub use resolver_selection::ResolverFactory;
