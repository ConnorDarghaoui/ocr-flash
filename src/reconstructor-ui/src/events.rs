// =============================================================================
// events — Consumidor del canal PipelineEvent para el CLI
//
// Propósito: Imprime progreso a stderr mientras el pipeline corre en el hilo
//            principal. Termina cuando recibe ProcesamientoCompleto o ErrorGlobal
//            (eventos terminales) o cuando el canal se cierra.
// =============================================================================

use std::sync::mpsc::Receiver;

use reconstructor_app::PipelineEvent;

/// Consume el canal de eventos e imprime progreso a stderr.
///
/// Diseñado para correr en un hilo separado mientras `PipelineOrchestrator::procesar`
/// corre en el hilo principal. Termina cuando llega un evento terminal o el canal cierra.
pub fn imprimir_eventos(rx: Receiver<PipelineEvent>) {
    for evento in rx {
        match evento {
            PipelineEvent::PaginaProgreso {
                num_pagina,
                bloques_terminados,
                bloques_totales,
            } => {
                eprintln!(
                    "  Página {num_pagina}: {bloques_terminados}/{bloques_totales} bloques"
                );
            }

            PipelineEvent::PaginaEstadoCambiada { num_pagina, estado } => {
                eprintln!("  Página {num_pagina}: {estado:?}");
            }

            PipelineEvent::ProcesamientoCompleto { metricas } => {
                eprintln!(
                    "✓ Completado en {:.1}s — {} páginas, {} bloques ({} texto, {} tabla, {} raster)",
                    metricas.tiempo_total_ms / 1000.0,
                    metricas.total_paginas,
                    metricas.total_bloques_detectados,
                    metricas.bloques_resueltos_texto,
                    metricas.bloques_resueltos_tabla,
                    metricas.bloques_fallback_raster,
                );
                break;
            }

            PipelineEvent::ErrorGlobal(msg) => {
                eprintln!("✗ Error global en pipeline: {msg}");
                break;
            }

            // Eventos de transición de estado detallados: omitidos en CLI básico.
            _ => {}
        }
    }
}
