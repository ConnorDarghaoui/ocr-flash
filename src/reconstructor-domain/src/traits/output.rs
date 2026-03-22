// =============================================================================
// OutputGenerator — Abstracción de I/O y serialización de artefactos
//
// Propósito: Desacoplar la lógica de escritura a disco de la lógica de negocio.
//            Permite añadir nuevos generadores (ej. Markdown, CSV) 
//            inyectándolos en la capa de Aplicación sin mutar el Orquestador (Sección 8.6 del SRS).
// =============================================================================

use crate::document::Document;
use crate::error::DomainError;
use crate::traits::composer::ComposedPage;

/// Contrato para la materialización persistente de los datos inferidos.
///
/// Protege al núcleo (Dominio/Aplicación) del contacto con librerías externas
/// (`serde_json`, `printpdf`, `std::fs`), dictando que cualquier error durante la exportación
/// debe ser absorbido o reportado a través del tipo seguro `DomainError`.
pub trait OutputGenerator: Send + Sync {
    /// Despliega en memoria persistente el artefacto correspondiente a su implementación.
    ///
    /// # Arguments
    ///
    /// * `documento` - Grafo de entidades del dominio, contiene el historial de estados
    ///   para auditoría de la FSM y los metadatos globales.
    /// * `paginas` - Vectores renderizados listos para ser ensamblados en formatos ricos 
    ///   o concatenados en texto plano.
    /// * `ruta_salida` - Prefijo canónico de salida. Las implementaciones añadirán su 
    ///   propia extensión (ej. `.pdf`, `.json`).
    ///
    /// # Errors
    ///
    /// Retorna `DomainError` si hay denegación de permisos de escritura por el SO,
    /// o si la estructura de datos agota la memoria del serializador.
    fn generar(
        &self,
        documento: &Document,
        paginas: &[ComposedPage],
        ruta_salida: &str,
    ) -> Result<(), DomainError>;
}
