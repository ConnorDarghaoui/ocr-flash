// =============================================================================
// Document, Page, PageImage — Agregados Raíz
//
// Propósito: Definir la jerarquía inmutable de datos estructurados que fluyen a través
//            del orquestador. Impide fugas de abstracción al aislar los blobs rasterizados
//            de la representación semántica final.
//
// Jerarquía: |> Document
//            |> Page
//            |> Region / ResolvedBlock
// =============================================================================

use serde::{Deserialize, Serialize};

use crate::metrics::ProcessingMetrics;
use crate::region::Region;
use crate::resolved::ResolvedBlock;

/// Contenedor efímero para el tráfico inter-hilos (inter-thread) del Event Bus.
///
/// Encapsula el buffer raster desvinculado del hardware (PDF decoder o filesystem),
/// permitiendo a `Rayon` (thread pool) mutar y clonar referencias a la imagen sin
/// colisiones de memoria.
#[derive(Debug, Clone)]
pub struct PageImage {
    pub datos: Vec<u8>,
    pub ancho: u32,
    pub alto: u32,
    pub numero_pagina: u32,
}

/// Nodo intermedio de la jerarquía que amalgama el grafo físico con el semántico.
///
/// Sirve como punto de verdad (Single Source of Truth) para el `PageComposer`, el cual
/// correlaciona los tensores espaciales originales (`regiones`) con los artefactos finales (`bloques_resueltos`)
/// a través de la identidad relacional (`Region::id`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    pub numero_pagina: u32,
    pub ancho: u32,
    pub alto: u32,
    pub orientacion_correccion_grados: f32,
    pub orientacion_incierta: bool,
    pub regiones: Vec<Region>,
    pub bloques_resueltos: Vec<ResolvedBlock>,
    pub tiempo_procesamiento_ms: f64,
}

/// Agregado raíz del dominio serializable.
///
/// Modela el "Receipt" final entregado al solicitante, satisfaciendo el 
/// requerimiento no funcional RNF07 (Trazabilidad). El documento es totalmente
/// agnóstico a los algoritmos que lo produjeron.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub ruta_origen: String,
    pub version_pipeline: String,
    pub procesado_en: String,
    pub paginas: Vec<Page>,
    pub metricas: ProcessingMetrics,
}
