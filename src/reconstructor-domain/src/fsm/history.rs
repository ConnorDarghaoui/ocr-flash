// =============================================================================
// StateTransition — Traza de auditoría de transiciones FSM
//
// Propósito: Habilita la observabilidad estricta y reconstrucción post-mortem
//            (Sección 7 del SRS). Permite deducir exactamente por qué un bloque 
//            terminó en fallback.
// =============================================================================

use serde::{Deserialize, Serialize};

/// Estructura inmutable proyectada en el reporte final JSON.
///
/// Modela el historial temporal de saltos de estado del pipeline para cada bloque.
/// Crítico para la auditoría de calidad (ADR-009) sin depender de logs de consola.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransition {
    pub from: String,
    pub to: String,
    pub event: String,
    pub ms: f64,
}

impl StateTransition {
    /// Empaqueta una transición registrada para su posterior serialización JSON.
    ///
    /// # Arguments
    ///
    /// * `from` - String representativo del estado saliente.
    /// * `to` - String representativo del estado entrante.
    /// * `event` - Identificador legible del disparador de la mutación.
    /// * `ms` - Marca de tiempo relativa a la instanciación de la región.
    pub fn new(
        from: impl Into<String>,
        to: impl Into<String>,
        event: impl Into<String>,
        ms: f64,
    ) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            event: event.into(),
            ms,
        }
    }
}
