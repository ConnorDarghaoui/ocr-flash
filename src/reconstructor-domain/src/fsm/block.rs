// =============================================================================
// BlockFSM — Autómata de resolución de región individual (Nivel 3)
//
// Propósito: Encapsula la lógica de reintentos y fallback a nivel granular (ADR-009).
//            Evita que un fallo de inferencia local aborte toda la composición.
//
// Flujo:     |> Detected
//            |> Resolving
//            |> LowConfidence (Evaluación de heurísticas)
//            |> Resolved (si éxito) o Fallback/StrategyError (si error)
//            |> Composed
// =============================================================================

use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::resolved::{ResolvedContent, StrategyKind};

/// Máquina de estados para la resolución concurrente de regiones aisladas.
///
/// Encapsula la lógica de reintentos y degradación segura (fallback) a nivel granular,
/// evitando que fallos de inferencia locales comprometan el procesamiento de la página completa (ADR-009).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum BlockState {
    Detected,
    Resolving {
        estrategia: StrategyKind,
    },
    LowConfidence {
        resultado: Box<ResolvedContent>,
        confianza: f32,
        retries_restantes: u32,
    },
    Resolved {
        contenido: Box<ResolvedContent>,
        confianza: f32,
        estrategia: StrategyKind,
    },
    StrategyError {
        error: String,
        puede_fallback: bool,
    },
    Fallback {
        crop_bytes: Vec<u8>,
    },
    /// Marcador para descartar silenciosamente regiones con geometrías inválidas
    /// sin abortar la composición del resto del documento.
    Unresolvable {
        razon: String,
    },
    Composed,
}

impl BlockState {
    pub fn es_terminal(&self) -> bool {
        matches!(self, Self::Composed | Self::Unresolvable { .. })
    }

    pub fn nombre(&self) -> &'static str {
        match self {
            Self::Detected => "Detected",
            Self::Resolving { .. } => "Resolving",
            Self::LowConfidence { .. } => "LowConfidence",
            Self::Resolved { .. } => "Resolved",
            Self::StrategyError { .. } => "StrategyError",
            Self::Fallback { .. } => "Fallback",
            Self::Unresolvable { .. } => "Unresolvable",
            Self::Composed => "Composed",
        }
    }
}

#[derive(Debug)]
pub enum BlockEvent {
    EstrategiaSeleccionada(StrategyKind),
    InferenciaOk {
        contenido: ResolvedContent,
        confianza: f32,
    },
    InferenciaFallida(String),
    ReintentoOk {
        contenido: ResolvedContent,
        confianza: f32,
    },
    ReintentoFallido,
    AplicarFallback {
        crop_bytes: Vec<u8>,
    },
    CropFallido,
    Compuesto,
}

impl BlockEvent {
    pub fn nombre(&self) -> String {
        match self {
            Self::EstrategiaSeleccionada(s) => format!("EstrategiaSeleccionada({s})"),
            Self::InferenciaOk { confianza, .. } => format!("InferenciaOk(conf={confianza:.2})"),
            Self::InferenciaFallida(e) => format!("InferenciaFallida({e})"),
            Self::ReintentoOk { confianza, .. } => format!("ReintentoOk(conf={confianza:.2})"),
            Self::ReintentoFallido => "ReintentoFallido".into(),
            Self::AplicarFallback { .. } => "AplicarFallback".into(),
            Self::CropFallido => "CropFallido".into(),
            Self::Compuesto => "Compuesto".into(),
        }
    }
}

/// Evalúa el evento contra el estado actual para avanzar el ciclo de vida de la región.
///
/// La inyección explícita de `confidence_threshold` evita acoplar el dominio a un singleton
/// de configuración, manteniendo la pureza de la función para pruebas unitarias deterministas.
///
/// # Arguments
///
/// * `estado` - Estado actual de la región.
/// * `evento` - Suceso proveniente del motor de inferencia u orquestador.
/// * `confidence_threshold` - Límite de aceptación; valores inferiores fuerzan reintentos o fallback.
///
/// # Errors
///
/// Retorna `DomainError::TransicionInvalida` si el flujo propuesto rompe la máquina de estados,
/// protegiendo la composición final de inconsistencias lógicas.
pub fn transition(
    estado: &BlockState,
    evento: BlockEvent,
    confidence_threshold: f32,
) -> Result<BlockState, DomainError> {
    match (estado, evento) {
        // Detected → Resolving: estrategia seleccionada
        (BlockState::Detected, BlockEvent::EstrategiaSeleccionada(estrategia)) => {
            Ok(BlockState::Resolving { estrategia })
        }

        // Resolving → Resolved: inferencia ok, confianza suficiente
        (
            BlockState::Resolving { .. },
            BlockEvent::InferenciaOk { contenido, confianza },
        ) if confianza >= confidence_threshold => Ok(BlockState::Resolved {
            contenido: Box::new(contenido),
            confianza,
            estrategia: match estado {
                BlockState::Resolving { estrategia } => *estrategia,
                _ => unreachable!(),
            },
        }),

        // Resolving → LowConfidence: inferencia ok, confianza baja
        (
            BlockState::Resolving { .. },
            BlockEvent::InferenciaOk { contenido, confianza },
        ) => Ok(BlockState::LowConfidence {
            resultado: Box::new(contenido),
            confianza,
            retries_restantes: 1,
        }),

        // Resolving → StrategyError: inferencia fallo
        (BlockState::Resolving { .. }, BlockEvent::InferenciaFallida(error)) => {
            Ok(BlockState::StrategyError {
                error,
                puede_fallback: true,
            })
        }

        // LowConfidence → Resolved: reintento exitoso
        (
            BlockState::LowConfidence { .. },
            BlockEvent::ReintentoOk { contenido, confianza },
        ) if confianza >= confidence_threshold => Ok(BlockState::Resolved {
            contenido: Box::new(contenido),
            confianza,
            estrategia: StrategyKind::Retry,
        }),

        // LowConfidence → Fallback: reintento fallido
        (
            BlockState::LowConfidence { .. },
            BlockEvent::ReintentoFallido,
        ) => Ok(BlockState::StrategyError {
            error: "Reintento fallido: confianza insuficiente tras retry".into(),
            puede_fallback: true,
        }),

        // LowConfidence → Fallback: ReintentoOk pero sigue baja confianza
        (
            BlockState::LowConfidence { .. },
            BlockEvent::ReintentoOk { .. },
        ) => Ok(BlockState::StrategyError {
            error: "Reintento completado pero confianza sigue por debajo del threshold".into(),
            puede_fallback: true,
        }),

        // StrategyError → Fallback: hay crop disponible
        (
            BlockState::StrategyError { puede_fallback: true, .. },
            BlockEvent::AplicarFallback { crop_bytes },
        ) => Ok(BlockState::Fallback { crop_bytes }),

        // StrategyError → Unresolvable: crop fallo o no hay fallback
        (
            BlockState::StrategyError { .. },
            BlockEvent::CropFallido,
        ) => Ok(BlockState::Unresolvable {
            razon: "Error en estrategia y crop fallido: bloque irrecuperable".into(),
        }),

        // Resolved → Composed
        (BlockState::Resolved { .. }, BlockEvent::Compuesto) => Ok(BlockState::Composed),

        // Fallback → Composed
        (BlockState::Fallback { .. }, BlockEvent::Compuesto) => Ok(BlockState::Composed),

        // Cualquier otra combinacion es invalida
        (estado, _) => Err(DomainError::TransicionInvalida {
            estado: estado.clone(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolved::ResolvedContent;

    const THRESHOLD: f32 = 0.60;

    fn contenido_texto() -> ResolvedContent {
        ResolvedContent::Text { texto: "hola mundo".into() }
    }

    fn crop_dummy() -> Vec<u8> {
        vec![0u8; 16]
    }

    // --- Happy path ---

    #[test]
    fn detected_estrategia_seleccionada_va_a_resolving() {
        let resultado = transition(
            &BlockState::Detected,
            BlockEvent::EstrategiaSeleccionada(StrategyKind::PaddleOcr),
            THRESHOLD,
        );
        assert!(matches!(
            resultado.unwrap(),
            BlockState::Resolving { estrategia: StrategyKind::PaddleOcr }
        ));
    }

    #[test]
    fn resolving_inferencia_ok_alta_confianza_va_a_resolved() {
        let estado = BlockState::Resolving { estrategia: StrategyKind::PaddleOcr };
        let resultado = transition(
            &estado,
            BlockEvent::InferenciaOk { contenido: contenido_texto(), confianza: 0.90 },
            THRESHOLD,
        );
        assert!(matches!(resultado.unwrap(), BlockState::Resolved { confianza, .. } if confianza == 0.90));
    }

    #[test]
    fn resolving_inferencia_ok_baja_confianza_va_a_lowconfidence() {
        let estado = BlockState::Resolving { estrategia: StrategyKind::PaddleOcr };
        let resultado = transition(
            &estado,
            BlockEvent::InferenciaOk { contenido: contenido_texto(), confianza: 0.40 },
            THRESHOLD,
        );
        assert!(matches!(resultado.unwrap(), BlockState::LowConfidence { confianza, .. } if confianza == 0.40));
    }

    #[test]
    fn resolving_inferencia_fallida_va_a_strategyerror() {
        let estado = BlockState::Resolving { estrategia: StrategyKind::PaddleOcr };
        let resultado = transition(
            &estado,
            BlockEvent::InferenciaFallida("OOM".into()),
            THRESHOLD,
        );
        assert!(matches!(resultado.unwrap(), BlockState::StrategyError { .. }));
    }

    #[test]
    fn lowconfidence_reintento_ok_va_a_resolved() {
        let estado = BlockState::LowConfidence {
            resultado: Box::new(contenido_texto()),
            confianza: 0.40,
            retries_restantes: 1,
        };
        let resultado = transition(
            &estado,
            BlockEvent::ReintentoOk { contenido: contenido_texto(), confianza: 0.75 },
            THRESHOLD,
        );
        assert!(matches!(
            resultado.unwrap(),
            BlockState::Resolved { estrategia: StrategyKind::Retry, .. }
        ));
    }

    #[test]
    fn lowconfidence_reintento_fallido_va_a_strategyerror() {
        let estado = BlockState::LowConfidence {
            resultado: Box::new(contenido_texto()),
            confianza: 0.40,
            retries_restantes: 0,
        };
        let resultado = transition(&estado, BlockEvent::ReintentoFallido, THRESHOLD);
        assert!(matches!(resultado.unwrap(), BlockState::StrategyError { .. }));
    }

    #[test]
    fn strategyerror_aplicar_fallback_va_a_fallback() {
        let estado = BlockState::StrategyError {
            error: "fallo".into(),
            puede_fallback: true,
        };
        let resultado = transition(
            &estado,
            BlockEvent::AplicarFallback { crop_bytes: crop_dummy() },
            THRESHOLD,
        );
        assert!(matches!(resultado.unwrap(), BlockState::Fallback { .. }));
    }

    #[test]
    fn strategyerror_crop_fallido_va_a_unresolvable() {
        let estado = BlockState::StrategyError {
            error: "fallo".into(),
            puede_fallback: true,
        };
        let resultado = transition(&estado, BlockEvent::CropFallido, THRESHOLD);
        assert!(matches!(resultado.unwrap(), BlockState::Unresolvable { .. }));
    }

    #[test]
    fn resolved_compuesto_va_a_composed() {
        let estado = BlockState::Resolved {
            contenido: Box::new(contenido_texto()),
            confianza: 0.85,
            estrategia: StrategyKind::PaddleOcr,
        };
        let resultado = transition(&estado, BlockEvent::Compuesto, THRESHOLD);
        assert_eq!(resultado.unwrap(), BlockState::Composed);
    }

    #[test]
    fn fallback_compuesto_va_a_composed() {
        let estado = BlockState::Fallback { crop_bytes: crop_dummy() };
        let resultado = transition(&estado, BlockEvent::Compuesto, THRESHOLD);
        assert_eq!(resultado.unwrap(), BlockState::Composed);
    }

    // --- Terminales ---

    #[test]
    fn composed_es_terminal() {
        assert!(BlockState::Composed.es_terminal());
    }

    #[test]
    fn unresolvable_es_terminal() {
        assert!(BlockState::Unresolvable { razon: "test".into() }.es_terminal());
    }

    #[test]
    fn detected_no_es_terminal() {
        assert!(!BlockState::Detected.es_terminal());
    }

    // --- Transiciones invalidas ---

    #[test]
    fn detected_evento_invalido_retorna_error() {
        let resultado = transition(&BlockState::Detected, BlockEvent::Compuesto, THRESHOLD);
        assert!(resultado.is_err());
    }

    #[test]
    fn composed_no_acepta_eventos() {
        let resultado = transition(
            &BlockState::Composed,
            BlockEvent::EstrategiaSeleccionada(StrategyKind::PaddleOcr),
            THRESHOLD,
        );
        assert!(resultado.is_err());
    }

    // --- Threshold en la frontera ---

    #[test]
    fn confianza_exactamente_en_threshold_va_a_resolved() {
        let estado = BlockState::Resolving { estrategia: StrategyKind::PaddleOcr };
        let resultado = transition(
            &estado,
            BlockEvent::InferenciaOk { contenido: contenido_texto(), confianza: THRESHOLD },
            THRESHOLD,
        );
        assert!(matches!(resultado.unwrap(), BlockState::Resolved { .. }));
    }

    #[test]
    fn confianza_justo_debajo_del_threshold_va_a_lowconfidence() {
        let estado = BlockState::Resolving { estrategia: StrategyKind::PaddleOcr };
        let resultado = transition(
            &estado,
            BlockEvent::InferenciaOk { contenido: contenido_texto(), confianza: THRESHOLD - 0.001 },
            THRESHOLD,
        );
        assert!(matches!(resultado.unwrap(), BlockState::LowConfidence { .. }));
    }
}
