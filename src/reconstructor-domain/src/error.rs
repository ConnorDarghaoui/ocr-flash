// =============================================================================
// DomainError — Errores tipados del dominio
//
// Proposito: Errores que puede producir la logica de dominio pura.
//            No incluye errores de infraestructura (I/O, ONNX, etc.),
//            esos se definen en reconstructor-infra.
// Dependencias: thiserror
// =============================================================================

use thiserror::Error;

use crate::block_type::BlockType;
use crate::fsm::block::BlockState;

/// Errores del dominio del sistema OCR.
///
/// Estos errores representan violaciones de invariantes de negocio,
/// no fallos de infraestructura. Los adapters del anillo 4 tienen
/// sus propios tipos de error que se mapean a estos cuando cruzan
/// la frontera dominio-infraestructura.
#[derive(Debug, Error)]
pub enum DomainError {
    /// Transicion de estado invalida en el FSM.
    #[error("Transicion invalida: estado={estado:?}, no se puede procesar el evento en este estado")]
    TransicionInvalida {
        /// Estado actual del FSM al momento del error.
        estado: BlockState,
    },

    /// Tipo de bloque no tiene estrategia de resolucion asignada.
    #[error("Sin estrategia de resolucion para tipo de bloque: {tipo_bloque}")]
    SinEstrategia {
        /// Tipo de bloque sin resolver asignado.
        tipo_bloque: BlockType,
    },

    /// Configuracion invalida detectada en dominio.
    #[error("Configuracion invalida: {mensaje}")]
    ConfiguracionInvalida {
        /// Descripcion del problema de configuracion.
        mensaje: String,
    },

    /// Documento de entrada vacio (sin paginas).
    #[error("El documento de entrada no contiene paginas")]
    DocumentoVacio,

    /// Region con coordenadas invalidas.
    #[error("Region '{region_id}' tiene coordenadas invalidas: {detalle}")]
    RegionInvalida {
        /// ID de la region problematica.
        region_id: String,
        /// Detalle del problema geometrico.
        detalle: String,
    },

    /// Transicion de estado invalida en el PageFSM.
    #[error("Transicion invalida en PageFSM: estado='{estado}', evento no valido en este estado")]
    TransicionInvalidaPagina {
        /// Nombre del estado actual del PageFSM.
        estado: String,
    },

    /// Transicion de estado invalida en el DocumentFSM.
    #[error("Transicion invalida en DocumentFSM: estado='{estado}', evento no valido en este estado")]
    TransicionInvalidaDocumento {
        /// Nombre del estado actual del DocumentFSM.
        estado: String,
    },

    /// Error originado en la capa de infraestructura (I/O, ONNX, PDF, red).
    #[error("Error de infraestructura: {0}")]
    Infraestructura(String),
}
