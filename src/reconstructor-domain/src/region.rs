// =============================================================================
// Region — Representación unificada de entidades extraídas
//
// Propósito: Define la unidad mínima atómica de la arquitectura (ADR-004). 
//            Encapsula los metadatos necesarios para instanciar el BlockFSM
//            independizando a las estrategias concretas del contexto de origen de la página.
// =============================================================================

use serde::{Deserialize, Serialize};

use crate::bbox::BoundingBox;
use crate::block_type::BlockType;

/// Entidad fundamental inyectada a la fábrica de resolutores (ResolverFactory).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Region {
    pub id: String,
    pub tipo_bloque: BlockType,
    pub bbox: BoundingBox,
    pub confianza_deteccion: f32,
}

impl Region {
    pub fn new(
        id: impl Into<String>,
        tipo_bloque: BlockType,
        bbox: BoundingBox,
        confianza_deteccion: f32,
    ) -> Self {
        Self {
            id: id.into(),
            tipo_bloque,
            bbox,
            confianza_deteccion,
        }
    }
}
