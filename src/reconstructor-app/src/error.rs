// =============================================================================
// AppError — Errores de la capa de Application Services
//
// Proposito: Errores que puede producir el PipelineOrchestrator y el JobManager.
//            Distingue entre errores de dominio (logica de negocio) y errores
//            de pipeline (fallo en una etapa especifica).
// Dependencias: thiserror, DomainError del dominio
// =============================================================================

use reconstructor_domain::DomainError;
use thiserror::Error;

/// Errores de la capa de orquestacion del pipeline.
#[derive(Debug, Error)]
pub enum AppError {
    /// Error propagado desde la logica de dominio.
    #[error("Error de dominio: {0}")]
    Domain(#[from] DomainError),

    /// Fallo en una etapa especifica del pipeline.
    #[error("Error en etapa '{etapa}': {detalle}")]
    Pipeline {
        /// Nombre de la etapa que fallo (orientacion, layout, ocr, composicion, salida).
        etapa: String,
        /// Descripcion del fallo.
        detalle: String,
    },

    /// El documento no tiene ninguna pagina procesable tras el pipeline.
    #[error("El documento no tiene paginas validas tras el procesamiento")]
    SinPaginasValidas,
}

impl AppError {
    /// Crea un error de pipeline con etapa y detalle.
    pub fn pipeline(etapa: impl Into<String>, detalle: impl Into<String>) -> Self {
        Self::Pipeline {
            etapa: etapa.into(),
            detalle: detalle.into(),
        }
    }
}
