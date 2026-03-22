// =============================================================================
// downloader — Descarga de modelos con progreso en UI (F3.1)
//
// Throttle: máximo 10 actualizaciones/segundo por modelo (100ms).
// Al finalizar cada modelo actualiza su status a "ok".
// Al terminar todos navega a FileSelect (página 2).
// =============================================================================

use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use slint::{Model, VecModel};

use crate::{paths::AppPaths, AppWindow, ModelRowData};

pub fn iniciar_descarga(window: slint::Weak<AppWindow>, paths: AppPaths) {
    std::thread::spawn(move || {
        use reconstructor_domain::ModelProvider;
        use reconstructor_infra::{HuggingFaceModelProvider, ModelManifest};

        let entries =
            match ModelManifest::cargar_con_base_dir(&paths.manifest, &paths.models_dir) {
                Ok(e) => e,
                Err(err) => {
                    let msg: slint::SharedString =
                        format!("Error cargando manifest: {err}").into();
                    let w = window.clone();
                    slint::invoke_from_event_loop(move || {
                        if let Some(win) = w.upgrade() {
                            win.set_download_error(msg);
                            win.set_download_failed(true);
                            win.set_download_in_progress(false);
                        }
                    })
                    .ok();
                    return;
                }
            };

        // Marcar descarga en progreso
        {
            let w = window.clone();
            slint::invoke_from_event_loop(move || {
                if let Some(win) = w.upgrade() {
                    win.set_download_in_progress(true);
                    win.set_download_failed(false);
                    win.set_download_error("".into());
                }
            })
            .ok();
        }

        let provider = HuggingFaceModelProvider;
        let total = entries.len();

        for (model_idx, entry) in entries.iter().enumerate() {
            // Crear directorio del modelo si no existe
            if let Some(parent) = std::path::Path::new(&entry.path_local).parent() {
                let _ = std::fs::create_dir_all(parent);
            }

            // Actualizar status del modelo a "downloading"
            {
                let w = window.clone();
                let id: slint::SharedString = entry.id.clone().into();
                slint::invoke_from_event_loop(move || {
                    actualizar_status_modelo(&w, &id, "downloading", 0.0);
                })
                .ok();
            }

            // Throttle: 100ms entre actualizaciones de UI
            let ultimo_update = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(1)));
            let w_prog = window.clone();
            let id_prog: slint::SharedString = entry.id.clone().into();

            let result = provider.descargar_modelo(
                entry,
                Box::new(move |prog| {
                    let mut ultimo = ultimo_update.lock().unwrap();
                    if ultimo.elapsed() < Duration::from_millis(100) {
                        return;
                    }
                    *ultimo = Instant::now();

                    let fraccion = prog.fraccion() as f32;
                    let w = w_prog.clone();
                    let id = id_prog.clone();
                    slint::invoke_from_event_loop(move || {
                        actualizar_status_modelo(&w, &id, "downloading", fraccion);
                    })
                    .ok();
                }),
            );

            match result {
                Ok(()) => {
                    let w = window.clone();
                    let id: slint::SharedString = entry.id.clone().into();
                    let total_progress = (model_idx + 1) as f32 / total as f32;
                    slint::invoke_from_event_loop(move || {
                        actualizar_status_modelo(&w, &id, "ok", 1.0);
                        if let Some(win) = w.upgrade() {
                            win.set_download_total_progress(total_progress);
                        }
                    })
                    .ok();
                }
                Err(err) => {
                    let msg: slint::SharedString =
                        format!("Error descargando {}: {err}", entry.nombre).into();
                    let w = window.clone();
                    let id: slint::SharedString = entry.id.clone().into();
                    slint::invoke_from_event_loop(move || {
                        actualizar_status_modelo(&w, &id, "missing", 0.0);
                        if let Some(win) = w.upgrade() {
                            win.set_download_error(msg);
                            win.set_download_failed(true);
                            win.set_download_in_progress(false);
                        }
                    })
                    .ok();
                    return;
                }
            }
        }

        // Todo OK → navegar a FileSelect
        let w = window.clone();
        slint::invoke_from_event_loop(move || {
            if let Some(win) = w.upgrade() {
                win.set_download_in_progress(false);
                win.set_current_page(2);
            }
        })
        .ok();
    });
}

/// Actualiza progress y status de un modelo específico.
/// Reconstruye el VecModel completo (pocos modelos, 10 Hz máx → ok).
/// Debe llamarse dentro de invoke_from_event_loop.
fn actualizar_status_modelo(
    window: &slint::Weak<AppWindow>,
    id: &slint::SharedString,
    status: &str,
    progress: f32,
) {
    let Some(win) = window.upgrade() else { return };
    let models = win.get_models();
    let count = models.row_count();

    let mut rows: Vec<ModelRowData> = (0..count)
        .filter_map(|i| models.row_data(i))
        .collect();

    for row in rows.iter_mut() {
        if &row.id == id {
            row.status = status.into();
            row.progress = progress;
            break;
        }
    }

    win.set_models(Rc::new(VecModel::from(rows)).into());
}
