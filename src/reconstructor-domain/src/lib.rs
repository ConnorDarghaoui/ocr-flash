// =============================================================================
// reconstructor-domain — Anillos 1+2 de la Onion Architecture
//
// Proposito: Entidades puras del dominio, traits que actuan como ports,
//            FSMs de 3 niveles y domain services.
// Dependencias: Ninguna externa salvo serde (optional) y thiserror.
// =============================================================================

pub mod bbox;
pub mod block_type;
pub mod config;
pub mod document;
pub mod error;
pub mod eval;
pub mod fsm;
pub mod report;
pub mod metrics;
pub mod region;
pub mod resolved;
pub mod services;
pub mod traits;

// --- Re-exports de conveniencia ---

// Anillo 1 — Entidades
pub use bbox::BoundingBox;
pub use block_type::BlockType;
pub use config::{GeneralConfig, PipelineConfig};
pub use document::{Document, Page, PageImage};
pub use error::DomainError;
pub use eval::{cer, fallback_rate, iou, layout_iou, throughput_paginas_por_segundo, EvalReport};
pub use report::generar_informe_html;
pub use metrics::ProcessingMetrics;
pub use region::Region;
pub use resolved::{ResolvedBlock, ResolvedContent, StrategyKind, TableData};

// Anillo 1 — FSMs
pub use fsm::block::{BlockEvent, BlockState};
pub use fsm::document::{DocumentEvent, DocumentState, DownloadProgress, OutputStage};
pub use fsm::history::StateTransition;
pub use fsm::page::{PageEvent, PageState};
pub use fsm::transition::{transition_block, transition_document, transition_page};

// Anillo 1 — Traits (Ports)
pub use traits::{
    BlockResolver, ComposedPage, LayoutDetector, ModelEntry, ModelProvider, ModelStatus,
    OrientationCorrector, OrientationResult, OutputGenerator, PageComposer, TextlineOrientationCorrector,
};

// Anillo 2 — Domain Services
pub use services::{ConfidenceDecision, ConfidenceEvaluator, ResolverFactory};
