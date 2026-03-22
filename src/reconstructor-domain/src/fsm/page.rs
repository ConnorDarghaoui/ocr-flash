// =============================================================================
// PageFSM — Autómata concurrente a nivel de página (Nivel 2)
//
// Propósito: Impone un orden estricto en el procesamiento intra-página 
//            debido a dependencias de datos. Implementa degradación segura (RNF06)
//            permitiendo fallos en orientación sin abortar la página.
//
// Flujo:     |> Orienting
//            |> DetectingLayout
//            |> ResolvingBlocks (Punto de sincronización M-Bloques)
//            |> Composing
// =============================================================================

use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::fsm::block::BlockState;

/// Máquina de estados para la orquestación secuencial a nivel de página.
///
/// Impone un orden estricto en el procesamiento (orientación -> layout -> composición)
/// necesario debido a las dependencias de datos entre etapas, al mismo tiempo que provee 
/// una barrera de sincronización (`ResolvingBlocks`) para el procesamiento concurrente de sus bloques.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum PageState {
    Pending,
    Orienting,
    /// Preserva la página en el pipeline a pesar del fallo de la red neuronal de orientación.
    /// Implementa el requisito de degradación segura (RNF06) para no perder los datos del escaneo.
    Degraded {
        razon: String,
    },
    DetectingLayout,
    /// Actúa como barrera de sincronización (join point) para las instancias
    /// concurrentes del autómata de resolución de bloques (Nivel 3).
    ResolvingBlocks {
        total_bloques: usize,
        bloques_terminados: usize,
    },
    Composing,
    Done,
    Error {
        razon: String,
    },
}

impl PageState {
    pub fn es_terminal(&self) -> bool {
        matches!(self, Self::Done | Self::Error { .. })
    }

    pub fn nombre(&self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Orienting => "Orienting",
            Self::Degraded { .. } => "Degraded",
            Self::DetectingLayout => "DetectingLayout",
            Self::ResolvingBlocks { .. } => "ResolvingBlocks",
            Self::Composing => "Composing",
            Self::Done => "Done",
            Self::Error { .. } => "Error",
        }
    }
}

#[derive(Debug)]
pub enum PageEvent {
    IniciarProcesamiento,
    OrientacionCompletada {
        angulo_grados: f32,
    },
    OrientacionFallida {
        razon: String,
    },
    ContinuarDegradado,
    LayoutDetectado {
        num_regiones: usize,
    },
    LayoutFallido {
        razon: String,
    },
    BloqueTerminado {
        estado_final: BlockState,
    },
    TodosLosBloquesTerminados,
    ComposicionCompletada,
    ComposicionFallida {
        razon: String,
    },
}

impl PageEvent {
    pub fn nombre(&self) -> String {
        match self {
            Self::IniciarProcesamiento => "IniciarProcesamiento".into(),
            Self::OrientacionCompletada { angulo_grados } => {
                format!("OrientacionCompletada({}°)", angulo_grados)
            }
            Self::OrientacionFallida { razon } => format!("OrientacionFallida({razon})"),
            Self::ContinuarDegradado => "ContinuarDegradado".into(),
            Self::LayoutDetectado { num_regiones } => {
                format!("LayoutDetectado({num_regiones} regiones)")
            }
            Self::LayoutFallido { razon } => format!("LayoutFallido({razon})"),
            Self::BloqueTerminado { .. } => "BloqueTerminado".into(),
            Self::TodosLosBloquesTerminados => "TodosLosBloquesTerminados".into(),
            Self::ComposicionCompletada => "ComposicionCompletada".into(),
            Self::ComposicionFallida { razon } => format!("ComposicionFallida({razon})"),
        }
    }
}

/// Evalúa un evento contra el estado actual de la página para determinar la siguiente fase.
///
/// La pureza de la función aísla las reglas de negocio del orquestador asíncrono,
/// garantizando que las mutaciones de estado no estén acopladas a la ejecución de I/O.
///
/// # Arguments
///
/// * `estado` - Estado actual de la página.
/// * `evento` - Notificación de progreso de la capa de infraestructura.
///
/// # Errors
///
/// Retorna `DomainError::TransicionInvalidaPagina` si el evento viola la secuencialidad
/// esperada por el pipeline.
pub fn transition(estado: &PageState, evento: PageEvent) -> Result<PageState, DomainError> {
    match (estado, evento) {
        // Pending → Orienting: iniciar procesamiento
        (PageState::Pending, PageEvent::IniciarProcesamiento) => Ok(PageState::Orienting),

        // Orienting → DetectingLayout: orientacion ok
        (PageState::Orienting, PageEvent::OrientacionCompletada { .. }) => {
            Ok(PageState::DetectingLayout)
        }

        // Orienting → Degraded: orientacion fallo (degradacion segura)
        (PageState::Orienting, PageEvent::OrientacionFallida { razon }) => {
            Ok(PageState::Degraded { razon })
        }

        // Degraded → DetectingLayout: continuar sin correccion
        (PageState::Degraded { .. }, PageEvent::ContinuarDegradado) => {
            Ok(PageState::DetectingLayout)
        }

        // DetectingLayout → ResolvingBlocks: layout detectado
        (PageState::DetectingLayout, PageEvent::LayoutDetectado { num_regiones }) => {
            Ok(PageState::ResolvingBlocks {
                total_bloques: num_regiones,
                bloques_terminados: 0,
            })
        }

        // DetectingLayout → Error: layout fallo totalmente
        (PageState::DetectingLayout, PageEvent::LayoutFallido { razon }) => {
            Ok(PageState::Error { razon })
        }

        // ResolvingBlocks: un bloque termino, actualizar contador
        (
            PageState::ResolvingBlocks { total_bloques, bloques_terminados },
            PageEvent::BloqueTerminado { .. },
        ) => Ok(PageState::ResolvingBlocks {
            total_bloques: *total_bloques,
            bloques_terminados: bloques_terminados + 1,
        }),

        // ResolvingBlocks → Composing: todos los bloques terminaron
        (
            PageState::ResolvingBlocks { .. },
            PageEvent::TodosLosBloquesTerminados,
        ) => Ok(PageState::Composing),

        // Composing → Done: composicion exitosa
        (PageState::Composing, PageEvent::ComposicionCompletada) => Ok(PageState::Done),

        // Composing → Error: composicion fallo
        (PageState::Composing, PageEvent::ComposicionFallida { razon }) => {
            Ok(PageState::Error { razon })
        }

        // Cualquier otra combinacion es invalida
        (estado, _) => Err(DomainError::TransicionInvalidaPagina {
            estado: estado.nombre().into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Happy path completo ---

    #[test]
    fn flujo_completo_sin_degradacion() {
        let s0 = PageState::Pending;

        let s1 = transition(&s0, PageEvent::IniciarProcesamiento).unwrap();
        assert_eq!(s1, PageState::Orienting);

        let s2 = transition(
            &s1,
            PageEvent::OrientacionCompletada { angulo_grados: 0.0 },
        )
        .unwrap();
        assert_eq!(s2, PageState::DetectingLayout);

        let s3 = transition(&s2, PageEvent::LayoutDetectado { num_regiones: 3 }).unwrap();
        assert!(matches!(
            s3,
            PageState::ResolvingBlocks { total_bloques: 3, bloques_terminados: 0 }
        ));

        let s4 = transition(&s3, PageEvent::TodosLosBloquesTerminados).unwrap();
        assert_eq!(s4, PageState::Composing);

        let s5 = transition(&s4, PageEvent::ComposicionCompletada).unwrap();
        assert_eq!(s5, PageState::Done);
        assert!(s5.es_terminal());
    }

    // --- Degradacion segura ---

    #[test]
    fn orientacion_fallida_produce_degraded() {
        let estado = PageState::Orienting;
        let resultado = transition(
            &estado,
            PageEvent::OrientacionFallida { razon: "modelo fallo".into() },
        )
        .unwrap();
        assert!(matches!(resultado, PageState::Degraded { .. }));
    }

    #[test]
    fn degraded_continua_a_detecting_layout() {
        let estado = PageState::Degraded { razon: "test".into() };
        let resultado = transition(&estado, PageEvent::ContinuarDegradado).unwrap();
        assert_eq!(resultado, PageState::DetectingLayout);
    }

    // --- Errores ---

    #[test]
    fn layout_fallido_produce_error_terminal() {
        let estado = PageState::DetectingLayout;
        let resultado = transition(
            &estado,
            PageEvent::LayoutFallido { razon: "sin regiones".into() },
        )
        .unwrap();
        assert!(matches!(resultado, PageState::Error { .. }));
        assert!(resultado.es_terminal());
    }

    #[test]
    fn composicion_fallida_produce_error_terminal() {
        let estado = PageState::Composing;
        let resultado = transition(
            &estado,
            PageEvent::ComposicionFallida { razon: "disco lleno".into() },
        )
        .unwrap();
        assert!(matches!(resultado, PageState::Error { .. }));
        assert!(resultado.es_terminal());
    }

    // --- Transiciones invalidas ---

    #[test]
    fn done_no_acepta_eventos() {
        let resultado = transition(&PageState::Done, PageEvent::IniciarProcesamiento);
        assert!(resultado.is_err());
    }

    #[test]
    fn pending_evento_incorrecto_retorna_error() {
        let resultado = transition(&PageState::Pending, PageEvent::ComposicionCompletada);
        assert!(resultado.is_err());
    }

    // --- Terminales ---

    #[test]
    fn done_es_terminal() {
        assert!(PageState::Done.es_terminal());
    }

    #[test]
    fn error_es_terminal() {
        assert!(PageState::Error { razon: "test".into() }.es_terminal());
    }

    #[test]
    fn pending_no_es_terminal() {
        assert!(!PageState::Pending.es_terminal());
    }

    // --- Contador de bloques ---

    #[test]
    fn bloque_terminado_incrementa_contador() {
        let estado = PageState::ResolvingBlocks {
            total_bloques: 3,
            bloques_terminados: 1,
        };
        let resultado = transition(
            &estado,
            PageEvent::BloqueTerminado { estado_final: BlockState::Composed },
        )
        .unwrap();
        assert!(matches!(
            resultado,
            PageState::ResolvingBlocks { total_bloques: 3, bloques_terminados: 2 }
        ));
    }
}
