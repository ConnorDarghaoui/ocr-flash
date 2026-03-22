// =============================================================================
// ProcessingMetrics — Observabilidad y Telemetría del Batch
//
// Propósito: Define el contrato inmutable de telemetría emitido al culminar el 
//            pipeline, satisfaciendo los requerimientos de auditoría y QoS 
//            (Quality of Service) del sistema (RF15).
// =============================================================================

use serde::{Deserialize, Serialize};

/// Consolidado analítico acumulado por el `PipelineOrchestrator` post-ejecución.
///
/// Previene que la interfaz gráfica tenga que realizar reducciones de arrays complejos 
/// para computar estadísticas.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProcessingMetrics {
    pub total_paginas: u32,
    pub total_bloques_detectados: u32,
    pub bloques_resueltos_texto: u32,
    pub bloques_resueltos_tabla: u32,
    pub bloques_fallback_raster: u32,
    pub bloques_irresolubles: u32,
    pub tiempo_total_ms: f64,
    pub tiempo_promedio_por_pagina_ms: f64,
}
