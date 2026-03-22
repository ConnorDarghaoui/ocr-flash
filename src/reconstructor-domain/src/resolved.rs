// =============================================================================
// ResolvedBlock, ResolvedContent, TableData, StrategyKind
//
// Propósito: Define el contrato de salida inmutable que los resolvers 
//            entregan tras la inferencia espacial. Asegura que la capa 
//            de composición reciba artefactos estructurales independientes 
//            del modelo que los generó.
//
// Flujo:     |> [Inferencia de Región]
//            |> Empaquetado Semántico (Text/Table/Raster)
//            |> Inyección de Traza de FSM
// =============================================================================

use serde::{Deserialize, Serialize};

use crate::bbox::BoundingBox;
use crate::block_type::BlockType;
use crate::fsm::block::BlockState;
use crate::fsm::history::StateTransition;

/// Declara la estrategia terminal inyectada para resolver un bloque (ADR-005).
///
/// Conservada en la métrica final para auditar qué algoritmo estadístico 
/// se usó si un bloque demanda reanálisis por el operador.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum StrategyKind {
    PaddleOcr,
    SlaNetTable,
    RasterPreserve,
    Retry,
}

impl std::fmt::Display for StrategyKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PaddleOcr => write!(f, "PaddleOcr"),
            Self::SlaNetTable => write!(f, "SlaNetTable"),
            Self::RasterPreserve => write!(f, "RasterPreserve"),
            Self::Retry => write!(f, "Retry"),
        }
    }
}

/// Modela las dependencias bidimensionales del analizador de tablas (SLANet+).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableData {
    pub cells: Vec<Vec<String>>,
    pub filas: u32,
    pub columnas: u32,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub celdas_bbox: Vec<BoundingBox>,
}

/// Encapsula de forma segura los diferentes tipos de memoria decodificada.
///
/// La segregación evita mutabilidad inesperada y restringe que una implementación 
/// `Text` contenga tensores `Raster` perdidos que agoten la RAM del Composer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "content_type", rename_all = "snake_case")]
pub enum ResolvedContent {
    Text {
        texto: String,
    },
    Table {
        datos: TableData,
    },
    Raster {
        #[serde(skip)]
        imagen_bytes: Vec<u8>,
        ancho: u32,
        alto: u32,
    },
}

/// Nodo central del documento finalizado con su auditoría inyectada.
///
/// Agrupa la salida estructural requerida por el generador visual (PDF) y 
/// la traza de transiciones requerida por el reporte de compliance (JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedBlock {
    pub region_id: String,
    pub tipo_bloque: BlockType,
    pub contenido: ResolvedContent,
    pub confianza_resolucion: f32,
    pub estrategia_utilizada: StrategyKind,
    pub estado_actual: BlockState,
    pub historial_estados: Vec<StateTransition>,
}
