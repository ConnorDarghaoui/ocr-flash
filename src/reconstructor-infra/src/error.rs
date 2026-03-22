// =============================================================================
// InfraError — Encapsulación de Errores del Adaptador (Anillo 4)
//
// Propósito: Ocultar los tipos de error nativos de las librerías subyacentes 
//            (como `ort::Error`, `image::ImageError`, `std::io::Error`) unificándolos 
//            bajo un enumerador único. Facilita la conversión automática hacia 
//            el `DomainError` al cruzar la frontera de la Arquitectura Cebolla.
// =============================================================================

use thiserror::Error;

use reconstructor_domain::DomainError;

/// Catálogo exhaustivo de fallos que pueden derivar de I/O o hardware.
#[derive(Debug, Error)]
pub enum InfraError {
    #[error("Error de imagen: {0}")]
    Imagen(String),

    #[error("Error de PDF: {0}")]
    Pdf(String),

    #[error("Error ONNX: {0}")]
    Onnx(String),

    #[error("Error de I/O: {0}")]
    Io(#[from] std::io::Error),

    #[error("Error de red: {0}")]
    Red(String),

    #[error("Hash incorrecto: esperado={esperado}, obtenido={obtenido}")]
    HashIncorrecto { esperado: String, obtenido: String },
}

impl From<InfraError> for DomainError {
    /// Transmuta incondicionalmente un error físico en un error lógico (Dominio).
    fn from(e: InfraError) -> Self {
        DomainError::Infraestructura(e.to_string())
    }
}
