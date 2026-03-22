// =============================================================================
// LayoutDetector — Puerto de Segmentación Espacial (ADR-004)
//
// Propósito: Ocultar los tensores y la lógica de detección de objetos (YOLO) 
//            al dominio. Transforma bytes crudos en entidades geométricas (`Region`) 
//            procesables de forma independiente y paralela.
// =============================================================================

use crate::error::DomainError;
use crate::region::Region;

/// Inversión de Control para el modelo de clasificación de geometría de documento.
///
/// Su responsabilidad primaria no es solo inferir, sino aplicar heurísticas post-inferencia
/// (NMS y Reading-Order) para que el Orquestador reciba un array estrictamente secuencial
/// y libre de solapamientos conflictivos.
pub trait LayoutDetector: Send + Sync {
    /// Desacopla la evaluación visual de una página del FSM.
    ///
    /// # Arguments
    ///
    /// * `imagen_bytes` - Payload raster codificado. Debe estar pre-rotado si se ejecutó el corrector.
    /// * `ancho` - Extensión horizontal para normalización y escalado de BoundingBoxes.
    /// * `alto` - Extensión vertical para cálculos geométricos.
    /// * `numero_pagina` - Semilla determinista inyectada para generar identificadores (`blk_P_N`) únicos
    ///   para trazabilidad (audit trailing).
    ///
    /// # Errors
    ///
    /// Retorna `DomainError` si el payload no es decodificable por el subsistema gráfico o 
    /// si la inicialización de sesión ONNX en la infraestructura subyacente colapsa.
    fn detectar(
        &self,
        imagen_bytes: &[u8],
        ancho: u32,
        alto: u32,
        numero_pagina: u32,
    ) -> Result<Vec<Region>, DomainError>;
}
