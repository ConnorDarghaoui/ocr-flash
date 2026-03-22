// =============================================================================
// BlockType — Clasificación Semántica (Inferencia Espacial)
//
// Propósito: Define el vocabulario de dominio que mapea las clases de salida 
//            del modelo de Layout (DocLayout-YOLO) con las estrategias de 
//            resolución (ADR-005) para aislar al orquestador del machine learning.
// =============================================================================

use serde::{Deserialize, Serialize};

/// Enumera la taxonomía de clases que el subsistema visual es capaz de predecir.
///
/// La `ResolverFactory` inspecciona este enumerador inmutable para enrutar el tensor
/// a la sub-red neuronal correspondiente, o derivarlo a un fallback raster si carece
/// de semántica textual pura.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockType {
    Text,
    Title,
    Table,
    Figure,
    List,
    Header,
    Footer,
    PageNumber,
    Caption,
    Formula,
    Unknown,
}

impl BlockType {
    /// Determina si la región requiere derivación al pipeline de NLP/OCR secuencial.
    pub fn es_textual(&self) -> bool {
        matches!(self, Self::Text | Self::Title | Self::List | Self::Caption)
    }

    /// Determina si la región carece de valor semántico parseable (imágenes puras)
    /// o si su ambigüedad demanda preservación de pixeles crudos.
    pub fn es_raster(&self) -> bool {
        matches!(self, Self::Figure | Self::Unknown)
    }

    /// Determina si la región demanda inferencia estructural bidimensional (Grillas).
    pub fn es_tabla(&self) -> bool {
        matches!(self, Self::Table)
    }
}

impl std::fmt::Display for BlockType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let etiqueta = match self {
            Self::Text => "text",
            Self::Title => "title",
            Self::Table => "table",
            Self::Figure => "figure",
            Self::List => "list",
            Self::Header => "header",
            Self::Footer => "footer",
            Self::PageNumber => "page_number",
            Self::Caption => "caption",
            Self::Formula => "formula",
            Self::Unknown => "unknown",
        };
        write!(f, "{}", etiqueta)
    }
}
