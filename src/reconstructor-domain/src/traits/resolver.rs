use crate::block_type::BlockType;
use crate::error::DomainError;
use crate::region::Region;
use crate::resolved::ResolvedContent;

/// Abstrae la extracción de contenido mediante el patrón Strategy (ref: ADR-005).
/// 
/// Aísla al dominio de las librerías de infraestructura pesadas (como `ort` o bindings de C++).
/// Esto permite que el orquestador resuelva páginas concurrentemente sin conocer 
/// qué modelo de inferencia (PaddleOCR, YOLO, etc.) se está ejecutando por debajo.
pub trait BlockResolver: Send + Sync {
    /// Utilizado por `ResolverFactory` durante el pipeline de la página para 
    /// enrutar el bloque al modelo de inferencia correcto según su clasificación.
    fn puede_resolver(&self, tipo: BlockType) -> bool;

    /// Ejecuta el forward-pass del modelo correspondiente sobre el recorte de la imagen.
    ///
    /// # Arguments
    ///
    /// * `crop_bytes` - Debe ser un slice de imagen codificada válido, generado previamente
    ///   por la fase de Layout para evitar procesar toda la página en memoria.
    fn resolver(
        &self,
        region: &Region,
        crop_bytes: &[u8],
    ) -> Result<(ResolvedContent, f32), DomainError>;
}
