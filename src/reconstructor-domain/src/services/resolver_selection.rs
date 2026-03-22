// =============================================================================
// ResolverFactory — Inyector dinámico de estrategias
//
// Propósito: Delega la selección de implementaciones concretas de inferencia 
//            en tiempo de ejecución basándose en la clasificación espacial del bloque.
//            Garantiza que la lógica de orquestación permanezca agnóstica a los modelos 
//            subyacentes (ADR-005).
// =============================================================================

use crate::block_type::BlockType;
use crate::traits::resolver::BlockResolver;

/// Registra y evalúa la cadena de estrategias de resolución disponibles (Chain of Responsibility).
///
/// La fábrica itera secuencialmente sobre los resolvers registrados en el "Composition Root".
/// El orden de inyección dicta la prioridad; un bloque tipo `Table` iterará hasta hacer
/// match con la implementación de `SlaNet` u otra que declare soportarlo.
pub struct ResolverFactory {
    resolvers: Vec<Box<dyn BlockResolver>>,
}

impl ResolverFactory {
    /// Inicializa la cadena de estrategias evaluables.
    ///
    /// # Arguments
    ///
    /// * `resolvers` - Lista inyectada de implementaciones del trait `BlockResolver`. El último
    ///   elemento debería actuar idealmente como un fallback universal (ej. `RasterFallbackResolver`).
    pub fn new(resolvers: Vec<Box<dyn BlockResolver>>) -> Self {
        Self { resolvers }
    }

    /// Busca la primera estrategia capaz de inferir sobre la clasificación semántica solicitada.
    ///
    /// # Arguments
    ///
    /// * `tipo` - Clasificación predictiva de la región (e.g. Texto, Tabla, Figura).
    ///
    /// # Returns
    ///
    /// Retorna una referencia a la estrategia a utilizar, o `None` si el Composition Root
    /// no proveyó una implementación compatible y fallaron los fallbacks.
    pub fn para_bloque(&self, tipo: BlockType) -> Option<&dyn BlockResolver> {
        self.resolvers
            .iter()
            .find(|r| r.puede_resolver(tipo))
            .map(|r| r.as_ref())
    }

    pub fn num_resolvers(&self) -> usize {
        self.resolvers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::DomainError;
    use crate::region::Region;
    use crate::resolved::ResolvedContent;
    use crate::bbox::BoundingBox;

    struct MockTextResolver;
    impl BlockResolver for MockTextResolver {
        fn puede_resolver(&self, tipo: BlockType) -> bool {
            tipo.es_textual()
        }
        fn resolver(&self, _: &Region, _: &[u8]) -> Result<(ResolvedContent, f32), DomainError> {
            Ok((ResolvedContent::Text { texto: "mock".into() }, 0.90))
        }
    }

    struct MockTableResolver;
    impl BlockResolver for MockTableResolver {
        fn puede_resolver(&self, tipo: BlockType) -> bool {
            tipo.es_tabla()
        }
        fn resolver(&self, _: &Region, _: &[u8]) -> Result<(ResolvedContent, f32), DomainError> {
            Ok((ResolvedContent::Text { texto: "tabla_mock".into() }, 0.85))
        }
    }

    struct MockRasterResolver;
    impl BlockResolver for MockRasterResolver {
        fn puede_resolver(&self, _tipo: BlockType) -> bool {
            true
        }
        fn resolver(&self, _: &Region, _: &[u8]) -> Result<(ResolvedContent, f32), DomainError> {
            Ok((ResolvedContent::Raster { imagen_bytes: vec![], ancho: 1, alto: 1 }, 1.0))
        }
    }

    fn factory() -> ResolverFactory {
        ResolverFactory::new(vec![
            Box::new(MockTextResolver),
            Box::new(MockTableResolver),
            Box::new(MockRasterResolver),
        ])
    }

    #[test]
    fn texto_selecciona_text_resolver() {
        let f = factory();
        let resolver = f.para_bloque(BlockType::Text);
        assert!(resolver.is_some());
        let region = Region::new("r1", BlockType::Text, BoundingBox::new(0.0, 0.0, 10.0, 10.0), 0.9);
        let (contenido, _) = resolver.unwrap().resolver(&region, &[]).unwrap();
        assert!(matches!(contenido, ResolvedContent::Text { .. }));
    }

    #[test]
    fn titulo_selecciona_text_resolver() {
        let f = factory();
        assert!(f.para_bloque(BlockType::Title).is_some());
    }

    #[test]
    fn tabla_selecciona_table_resolver() {
        let f = factory();
        let resolver = f.para_bloque(BlockType::Table);
        assert!(resolver.is_some());
        let region = Region::new("r2", BlockType::Table, BoundingBox::new(0.0, 0.0, 10.0, 10.0), 0.9);
        let (contenido, _) = resolver.unwrap().resolver(&region, &[]).unwrap();
        assert!(matches!(contenido, ResolvedContent::Text { .. }));
    }

    #[test]
    fn figura_selecciona_raster_resolver() {
        let f = factory();
        let resolver = f.para_bloque(BlockType::Figure);
        assert!(resolver.is_some());
        let region = Region::new("r3", BlockType::Figure, BoundingBox::new(0.0, 0.0, 10.0, 10.0), 0.9);
        let (contenido, _) = resolver.unwrap().resolver(&region, &[]).unwrap();
        assert!(matches!(contenido, ResolvedContent::Raster { .. }));
    }

    #[test]
    fn unknown_selecciona_raster_fallback() {
        let f = factory();
        assert!(f.para_bloque(BlockType::Unknown).is_some());
    }

    #[test]
    fn sin_resolvers_retorna_none() {
        let f = ResolverFactory::new(vec![]);
        assert!(f.para_bloque(BlockType::Text).is_none());
    }

    #[test]
    fn num_resolvers_correcto() {
        let f = factory();
        assert_eq!(f.num_resolvers(), 3);
    }

    #[test]
    fn prioridad_es_orden_de_registro() {
        let f = factory();
        let region = Region::new("r4", BlockType::Text, BoundingBox::new(0.0, 0.0, 10.0, 10.0), 0.9);
        let resolver = f.para_bloque(BlockType::Text).unwrap();
        let (contenido, _) = resolver.resolver(&region, &[]).unwrap();
        match contenido {
            ResolvedContent::Text { texto } => assert_eq!(texto, "mock"),
            _ => panic!("Se esperaba Text del MockTextResolver"),
        }
    }
}
