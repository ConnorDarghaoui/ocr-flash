// =============================================================================
// PipelineEvent — Canal de comunicacion Orchestrator → UI (Observer pattern)
//
// Proposito: Enum que modela todos los eventos que el PipelineOrchestrator
//            emite hacia la UI via un canal mpsc. La UI de Slint consume estos
//            eventos para mostrar progreso granular en tiempo real:
//            barra de progreso por documento, indicador por pagina, estado por bloque.
//
// Cada transicion de estado en cualquiera de los 3 niveles del autómata jerarquico
// (Document, Page, Block) produce un evento aqui (ADR-009, seccion 5.2.1 del SRS).
// =============================================================================

use reconstructor_domain::{BlockState, DocumentState, PageState, ProcessingMetrics};

/// Evento emitido por el PipelineOrchestrator hacia la UI.
///
/// Se transmite por `std::sync::mpsc::Sender<PipelineEvent>`.
/// La UI recibe por el `Receiver<PipelineEvent>` que se obtiene en `PipelineOrchestrator::new()`.
#[derive(Debug)]
pub enum PipelineEvent {
    // -------------------------------------------------------------------------
    // Nivel Document — DocumentFSM
    // -------------------------------------------------------------------------

    /// El DocumentFSM cambio de estado.
    DocumentoEstadoCambiado(DocumentState),

    /// Progreso de descarga de un modelo ONNX (estado DownloadingModels).
    ModelosProgreso {
        /// Nombre/ID del modelo en descarga.
        modelo: String,
        /// Bytes descargados hasta ahora.
        bytes_descargados: u64,
        /// Bytes totales del modelo.
        bytes_totales: u64,
    },

    // -------------------------------------------------------------------------
    // Nivel Page — PageFSM
    // -------------------------------------------------------------------------

    /// El PageFSM de una pagina cambio de estado.
    PaginaEstadoCambiada {
        /// Numero de pagina (1-indexed).
        num_pagina: usize,
        /// Nuevo estado de la pagina.
        estado: PageState,
    },

    /// Progreso de resolucion de bloques dentro de una pagina.
    PaginaProgreso {
        /// Numero de pagina (1-indexed).
        num_pagina: usize,
        /// Bloques que ya alcanzaron estado terminal.
        bloques_terminados: usize,
        /// Total de bloques en la pagina.
        bloques_totales: usize,
    },

    // -------------------------------------------------------------------------
    // Nivel Block — BlockFSM
    // -------------------------------------------------------------------------

    /// El BlockFSM de un bloque cambio de estado.
    BloqueEstadoCambiado {
        /// Numero de pagina del bloque (1-indexed).
        num_pagina: usize,
        /// ID del bloque (formato "blk_{page}_{index}").
        bloque_id: String,
        /// Estado anterior del bloque.
        desde: BlockState,
        /// Estado nuevo del bloque.
        hasta: BlockState,
    },

    // -------------------------------------------------------------------------
    // Finalizacion
    // -------------------------------------------------------------------------

    /// El procesamiento completo termino exitosamente.
    ProcesamientoCompleto {
        /// Metricas globales del procesamiento.
        metricas: ProcessingMetrics,
    },

    /// Error global irrecuperable (el procesamiento no puede continuar).
    ErrorGlobal(String),
}
