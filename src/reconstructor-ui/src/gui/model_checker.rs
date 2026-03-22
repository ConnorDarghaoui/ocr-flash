// =============================================================================
// model_checker — Verificación de modelos al inicio
//
// Lanza un thread que consulta el manifest y el estado de cada modelo,
// luego actualiza la UI con ModelRowData y navega a la página correcta.
// =============================================================================

use slint::VecModel;
use std::rc::Rc;

use crate::{paths::AppPaths, AppWindow, ModelRowData};

pub fn verificar_y_poblar(window: slint::Weak<AppWindow>, paths: AppPaths) {
    std::thread::spawn(move || {
        use reconstructor_domain::{ModelProvider, ModelStatus};
        use reconstructor_infra::{HuggingFaceModelProvider, ModelManifest};

        let entries = match ModelManifest::cargar_con_base_dir(&paths.manifest, &paths.models_dir)
        {
            Ok(e) => e,
            Err(err) => {
                tracing::warn!("No se pudo cargar manifest {:?}: {err}", paths.manifest);
                // Sin manifest → saltar directo a FileSelect
                let w = window.clone();
                slint::invoke_from_event_loop(move || {
                    if let Some(win) = w.upgrade() {
                        win.set_current_page(2);
                    }
                })
                .ok();
                return;
            }
        };

        let provider = HuggingFaceModelProvider;
        let statuses = provider.verificar_modelos(&entries);

        let model_rows: Vec<ModelRowData> = entries
            .iter()
            .map(|e| {
                let status = statuses
                    .iter()
                    .find(|(id, _)| id == &e.id)
                    .map(|(_, s)| s.clone())
                    .unwrap_or(ModelStatus::Faltante);
                ModelRowData {
                    id: e.id.clone().into(),
                    name: e.nombre.clone().into(),
                    size_mb: e.size_mb as i32,
                    progress: if matches!(status, ModelStatus::Ok) { 1.0 } else { 0.0 },
                    status: match status {
                        ModelStatus::Ok => "ok".into(),
                        ModelStatus::Faltante => "missing".into(),
                        ModelStatus::Corrupto => "corrupted".into(),
                    },
                }
            })
            .collect();

        let todos_ok = statuses.iter().all(|(_, s)| matches!(s, ModelStatus::Ok));

        slint::invoke_from_event_loop(move || {
            if let Some(win) = window.upgrade() {
                let model = Rc::new(VecModel::from(model_rows));
                win.set_models(model.into());
                win.set_current_page(if todos_ok { 2 } else { 1 });
            }
        })
        .ok();
    });
}
