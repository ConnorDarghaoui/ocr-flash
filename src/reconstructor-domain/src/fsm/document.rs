// =============================================================================
// DocumentFSM — Autómata finito del ciclo de vida completo (Nivel 1)
//
// Propósito: Gobierna el flujo general. Modela la provisión de dependencias
//            (modelos ML) como estados explícitos (ADR-010) para garantizar
//            una distribución plug-and-play.
//
// Flujo:     |> Idle
//            |> CheckingModels
//            |> DownloadingModels (si faltan)
//            |> Validating
//            |> Processing
//            |> Generating
//            |> Complete
// =============================================================================

use serde::{Deserialize, Serialize};

use crate::error::DomainError;

/// Desacopla la métrica de transferencia asíncrona (I/O) de la infraestructura 
/// para exponerla a la UI de forma inmutable vía eventos de progreso.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DownloadProgress {
    pub modelo: String,
    pub bytes_descargados: u64,
    pub bytes_totales: u64,
}

impl DownloadProgress {
    /// Previene pánicos por división por cero en caso de que la conexión TCP 
    /// aborte antes de resolver el encabezado `Content-Length`.
    pub fn fraccion(&self) -> f32 {
        if self.bytes_totales == 0 {
            return 0.0;
        }
        self.bytes_descargados as f32 / self.bytes_totales as f32
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum OutputStage {
    Pdf,
    Txt,
    Json,
}

/// Máquina de estados que define la topología raíz de la aplicación (Nivel 1).
///
/// Modela la provisión de dependencias en frío (modelos ML) como parte integral 
/// del flujo para garantizar una distribución *plug-and-play* sin depender de scripts externos (ADR-010).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum DocumentState {
    Idle,
    CheckingModels,
    DownloadingModels {
        progreso: DownloadProgress,
    },
    Validating {
        ruta_entrada: String,
    },
    Processing {
        total_paginas: usize,
        paginas_completadas: usize,
    },
    Generating {
        etapa: OutputStage,
    },
    Complete,
    Error {
        error: String,
        puede_reintentar: bool,
    },
}

impl DocumentState {
    pub fn nombre(&self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::CheckingModels => "CheckingModels",
            Self::DownloadingModels { .. } => "DownloadingModels",
            Self::Validating { .. } => "Validating",
            Self::Processing { .. } => "Processing",
            Self::Generating { .. } => "Generating",
            Self::Complete => "Complete",
            Self::Error { .. } => "Error",
        }
    }
}

#[derive(Debug)]
pub enum DocumentEvent {
    AppIniciada,
    ModelosListos,
    ModelosFaltantes {
        progreso: DownloadProgress,
    },
    ProgresoDescarga(DownloadProgress),
    DescargaCompletada,
    DescargaFallida(String),
    ArchivoSeleccionado(String),
    EntradaValida {
        num_paginas: usize,
    },
    EntradaInvalida(String),
    PaginaCompletada,
    TodasLasPaginasCompletadas,
    ErrorIrrecuperable(String),
    EtapaGeneracion(OutputStage),
    SalidasEscritas,
    EscrituraFallida(String),
    Reiniciar,
    Reintentar,
}

impl DocumentEvent {
    pub fn nombre(&self) -> String {
        match self {
            Self::AppIniciada => "AppIniciada".into(),
            Self::ModelosListos => "ModelosListos".into(),
            Self::ModelosFaltantes { .. } => "ModelosFaltantes".into(),
            Self::ProgresoDescarga(p) => {
                format!("ProgresoDescarga({}: {:.0}%)", p.modelo, p.fraccion() * 100.0)
            }
            Self::DescargaCompletada => "DescargaCompletada".into(),
            Self::DescargaFallida(e) => format!("DescargaFallida({e})"),
            Self::ArchivoSeleccionado(p) => format!("ArchivoSeleccionado({p})"),
            Self::EntradaValida { num_paginas } => format!("EntradaValida({num_paginas} pags)"),
            Self::EntradaInvalida(e) => format!("EntradaInvalida({e})"),
            Self::PaginaCompletada => "PaginaCompletada".into(),
            Self::TodasLasPaginasCompletadas => "TodasLasPaginasCompletadas".into(),
            Self::ErrorIrrecuperable(e) => format!("ErrorIrrecuperable({e})"),
            Self::EtapaGeneracion(e) => format!("EtapaGeneracion({e:?})"),
            Self::SalidasEscritas => "SalidasEscritas".into(),
            Self::EscrituraFallida(e) => format!("EscrituraFallida({e})"),
            Self::Reiniciar => "Reiniciar".into(),
            Self::Reintentar => "Reintentar".into(),
        }
    }
}

/// Resuelve el estado general del procesamiento en base a los eventos del orquestador.
///
/// Implementada como función pura sin side-effects para permitir el testeo 
/// determinista de todos los flujos de error posibles (Fase 0 del Roadmap).
///
/// # Arguments
///
/// * `estado` - Estado actual de la aplicación.
/// * `evento` - Inyección de un suceso interno o externo.
///
/// # Errors
///
/// Retorna `DomainError::TransicionInvalidaDocumento` si el flujo propuesto rompe 
/// el ciclo de vida del autómata, protegiendo al sistema contra corrupciones de ejecución.
pub fn transition(estado: &DocumentState, evento: DocumentEvent) -> Result<DocumentState, DomainError> {
    match (estado, evento) {
        // Idle → CheckingModels: app iniciada
        (DocumentState::Idle, DocumentEvent::AppIniciada) => Ok(DocumentState::CheckingModels),

        // CheckingModels → Idle: modelos OK
        (DocumentState::CheckingModels, DocumentEvent::ModelosListos) => Ok(DocumentState::Idle),

        // CheckingModels → DownloadingModels: faltan modelos
        (DocumentState::CheckingModels, DocumentEvent::ModelosFaltantes { progreso }) => {
            Ok(DocumentState::DownloadingModels { progreso })
        }

        // DownloadingModels: progreso actualizado
        (
            DocumentState::DownloadingModels { .. },
            DocumentEvent::ProgresoDescarga(progreso),
        ) => Ok(DocumentState::DownloadingModels { progreso }),

        // DownloadingModels → Idle: descarga completada
        (DocumentState::DownloadingModels { .. }, DocumentEvent::DescargaCompletada) => {
            Ok(DocumentState::Idle)
        }

        // DownloadingModels → Error: descarga fallo
        (DocumentState::DownloadingModels { .. }, DocumentEvent::DescargaFallida(error)) => {
            Ok(DocumentState::Error {
                error,
                puede_reintentar: true,
            })
        }

        // Idle → Validating: archivo seleccionado
        (DocumentState::Idle, DocumentEvent::ArchivoSeleccionado(ruta)) => {
            Ok(DocumentState::Validating { ruta_entrada: ruta })
        }

        // Validating → Processing: archivo valido
        (DocumentState::Validating { .. }, DocumentEvent::EntradaValida { num_paginas }) => {
            Ok(DocumentState::Processing {
                total_paginas: num_paginas,
                paginas_completadas: 0,
            })
        }

        // Validating → Error: archivo invalido
        (DocumentState::Validating { .. }, DocumentEvent::EntradaInvalida(error)) => {
            Ok(DocumentState::Error {
                error,
                puede_reintentar: false,
            })
        }

        // Processing: pagina completada, actualizar contador
        (
            DocumentState::Processing { total_paginas, paginas_completadas },
            DocumentEvent::PaginaCompletada,
        ) => Ok(DocumentState::Processing {
            total_paginas: *total_paginas,
            paginas_completadas: paginas_completadas + 1,
        }),

        // Processing → Generating: todas las paginas completadas
        (DocumentState::Processing { .. }, DocumentEvent::TodasLasPaginasCompletadas) => {
            Ok(DocumentState::Generating { etapa: OutputStage::Pdf })
        }

        // Processing → Error: error irrecuperable
        (DocumentState::Processing { .. }, DocumentEvent::ErrorIrrecuperable(error)) => {
            Ok(DocumentState::Error {
                error,
                puede_reintentar: false,
            })
        }

        // Generating: cambio de etapa
        (DocumentState::Generating { .. }, DocumentEvent::EtapaGeneracion(etapa)) => {
            Ok(DocumentState::Generating { etapa })
        }

        // Generating → Complete: todas las salidas escritas
        (DocumentState::Generating { .. }, DocumentEvent::SalidasEscritas) => {
            Ok(DocumentState::Complete)
        }

        // Generating → Error: escritura fallo
        (DocumentState::Generating { .. }, DocumentEvent::EscrituraFallida(error)) => {
            Ok(DocumentState::Error {
                error,
                puede_reintentar: false,
            })
        }

        // Complete → Idle: reiniciar
        (DocumentState::Complete, DocumentEvent::Reiniciar) => Ok(DocumentState::Idle),

        // Error → Idle: reiniciar
        (DocumentState::Error { .. }, DocumentEvent::Reiniciar) => Ok(DocumentState::Idle),

        // Error → CheckingModels: reintentar (verifica modelos de nuevo)
        (DocumentState::Error { puede_reintentar: true, .. }, DocumentEvent::Reintentar) => {
            Ok(DocumentState::CheckingModels)
        }

        // Cualquier otra combinacion es invalida
        (estado, _) => Err(DomainError::TransicionInvalidaDocumento {
            estado: estado.nombre().into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progreso_dummy() -> DownloadProgress {
        DownloadProgress {
            modelo: "test_model".into(),
            bytes_descargados: 50,
            bytes_totales: 100,
        }
    }

    // --- Primer arranque: modelos OK ---

    #[test]
    fn app_iniciada_va_a_checking_models() {
        let resultado = transition(&DocumentState::Idle, DocumentEvent::AppIniciada).unwrap();
        assert_eq!(resultado, DocumentState::CheckingModels);
    }

    #[test]
    fn modelos_listos_vuelve_a_idle() {
        let resultado =
            transition(&DocumentState::CheckingModels, DocumentEvent::ModelosListos).unwrap();
        assert_eq!(resultado, DocumentState::Idle);
    }

    // --- Primer arranque: descarga necesaria ---

    #[test]
    fn modelos_faltantes_va_a_downloading() {
        let resultado = transition(
            &DocumentState::CheckingModels,
            DocumentEvent::ModelosFaltantes { progreso: progreso_dummy() },
        )
        .unwrap();
        assert!(matches!(resultado, DocumentState::DownloadingModels { .. }));
    }

    #[test]
    fn descarga_completada_vuelve_a_idle() {
        let estado = DocumentState::DownloadingModels { progreso: progreso_dummy() };
        let resultado = transition(&estado, DocumentEvent::DescargaCompletada).unwrap();
        assert_eq!(resultado, DocumentState::Idle);
    }

    #[test]
    fn descarga_fallida_va_a_error_reintentar() {
        let estado = DocumentState::DownloadingModels { progreso: progreso_dummy() };
        let resultado = transition(
            &estado,
            DocumentEvent::DescargaFallida("sin internet".into()),
        )
        .unwrap();
        assert!(matches!(
            resultado,
            DocumentState::Error { puede_reintentar: true, .. }
        ));
    }

    // --- Flujo de procesamiento ---

    #[test]
    fn flujo_completo_de_procesamiento() {
        let s0 = DocumentState::Idle;

        let s1 = transition(&s0, DocumentEvent::ArchivoSeleccionado("doc.pdf".into())).unwrap();
        assert!(matches!(s1, DocumentState::Validating { .. }));

        let s2 = transition(&s1, DocumentEvent::EntradaValida { num_paginas: 3 }).unwrap();
        assert!(matches!(
            s2,
            DocumentState::Processing { total_paginas: 3, paginas_completadas: 0 }
        ));

        let s3 = transition(&s2, DocumentEvent::TodasLasPaginasCompletadas).unwrap();
        assert!(matches!(
            s3,
            DocumentState::Generating { etapa: OutputStage::Pdf }
        ));

        let s4 = transition(&s3, DocumentEvent::SalidasEscritas).unwrap();
        assert_eq!(s4, DocumentState::Complete);
    }

    #[test]
    fn entrada_invalida_va_a_error() {
        let estado = DocumentState::Validating { ruta_entrada: "bad.xyz".into() };
        let resultado = transition(
            &estado,
            DocumentEvent::EntradaInvalida("formato no soportado".into()),
        )
        .unwrap();
        assert!(matches!(resultado, DocumentState::Error { .. }));
    }

    #[test]
    fn complete_reiniciar_vuelve_a_idle() {
        let resultado =
            transition(&DocumentState::Complete, DocumentEvent::Reiniciar).unwrap();
        assert_eq!(resultado, DocumentState::Idle);
    }

    #[test]
    fn error_reintentar_va_a_checking_models() {
        let estado = DocumentState::Error {
            error: "sin internet".into(),
            puede_reintentar: true,
        };
        let resultado = transition(&estado, DocumentEvent::Reintentar).unwrap();
        assert_eq!(resultado, DocumentState::CheckingModels);
    }

    // --- Transiciones invalidas ---

    #[test]
    fn complete_no_acepta_eventos_invalidos() {
        let resultado =
            transition(&DocumentState::Complete, DocumentEvent::AppIniciada);
        assert!(resultado.is_err());
    }

    #[test]
    fn checking_models_no_acepta_archivo_seleccionado() {
        let resultado = transition(
            &DocumentState::CheckingModels,
            DocumentEvent::ArchivoSeleccionado("doc.pdf".into()),
        );
        assert!(resultado.is_err());
    }

    // --- DownloadProgress ---

    #[test]
    fn fraccion_descarga_correcta() {
        let p = DownloadProgress {
            modelo: "test".into(),
            bytes_descargados: 75,
            bytes_totales: 100,
        };
        assert!((p.fraccion() - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn fraccion_descarga_cero_bytes_totales_no_panics() {
        let p = DownloadProgress {
            modelo: "test".into(),
            bytes_descargados: 0,
            bytes_totales: 0,
        };
        assert_eq!(p.fraccion(), 0.0);
    }
}
