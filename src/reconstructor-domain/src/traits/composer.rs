// =============================================================================
// PageComposer — Abstracción de renderizado vectorial (Sección 8.5 del SRS)
//
// Propósito: Oculta la complejidad del renderizado PDF y transformación de 
//            coordenadas (pixeles a puntos) aislando la lógica de reconstrucción 
//            del orquestador principal.
// =============================================================================

use crate::document::Page;
use crate::error::DomainError;
use crate::resolved::ResolvedBlock;

/// Artefacto intermedio inmutable generado por la capa de renderizado.
///
/// Encapsula el blob binario aislado de la página para que el generador final
/// (`OutputGenerator`) pueda ensamblar de forma concurrente todas las hojas sin
/// lidiar con condiciones de carrera sobre el archivo en el sistema de archivos.
#[derive(Debug, Clone)]
pub struct ComposedPage {
    pub numero_pagina: u32,
    pub pdf_bytes: Vec<u8>,
    pub texto_extraido: String,
}

/// Contrato para el ensamblaje "from scratch" de los bloques resueltos.
///
/// Materializa la regla de negocio principal: "Reconstrucción pura vs PDF Sandwich".
/// Obliga a la implementación a dibujar vectores y texto directamente sobre un lienzo 
/// en blanco en base a los metadatos de las regiones de inferencia.
pub trait PageComposer: Send + Sync {
    /// Despliega el contenido resuelto en un layout físico (`ComposedPage`).
    ///
    /// # Arguments
    ///
    /// * `pagina` - Metadatos inmutables de la página para obtener límites y escalas espaciales.
    /// * `bloques` - Lista de entidades en estado terminal. La implementación es libre de
    ///   descartar bloques marcados como `Unresolvable`.
    ///
    /// # Errors
    ///
    /// Retorna `DomainError` si el motor de composición (ej. desbordamiento en la asignación 
    /// de grillas, codificación tipográfica) colapsa durante la transmutación.
    fn componer(
        &self,
        pagina: &Page,
        bloques: &[ResolvedBlock],
    ) -> Result<ComposedPage, DomainError>;
}
