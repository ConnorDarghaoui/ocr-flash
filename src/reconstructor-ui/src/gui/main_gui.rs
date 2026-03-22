// =============================================================================
// main_gui — Entry point del binario reconstructor-gui
//
// Responsabilidades:
//   1. Resolver AppPaths (manifest, models_dir, config multiplataforma)
//   2. Cargar UserConfig (config del usuario o defaults)
//   3. Inicializar el AppWindow de Slint
//   4. Conectar callbacks de la UI a AppState
//   5. Correr el event loop (bloquea hasta que el usuario cierra la ventana)
// =============================================================================

mod app_state;
mod config_store;
mod downloader;
mod model_checker;
mod processor;

// compose.rs lives in src/ (shared with CLI binary)
#[path = "../compose.rs"]
mod compose;

// paths.rs también es compartido
#[path = "../paths.rs"]
mod paths;

slint::include_modules!();

use app_state::AppState;
use config_store::UserConfig;
use paths::AppPaths;
use slint::ComponentHandle;

fn main() -> anyhow::Result<()> {
    // Resolver paths multiplataforma al arranque
    let app_paths = AppPaths::resolver();

    // Config del usuario (usa defaults si no existe)
    let config = UserConfig::cargar(&app_paths);

    // Crear ventana Slint (debe hacerse en el main thread)
    let window = AppWindow::new()?;

    // Estado compartido — Weak<AppWindow> es Clone + Send
    let state = AppState::new(window.as_weak(), config.clone());

    // Empezar en CheckingModels (página 0)
    window.set_current_page(0);

    // Directorio de salida inicial desde config
    window.set_output_dir(config.output_dir.clone().into());

    // Aplicar dark mode
    window.set_dark_mode(config.dark_mode);

    // Cargar settings iniciales en la UI
    window.set_current_settings(SettingsData {
        confidence_threshold: config.pipeline.ocr.confidence_threshold,
        dpi: config.pipeline.input.rasterization_dpi as i32,
        ocr_language: "es".into(),
        output_dir: config.output_dir.clone().into(),
        dark_mode: config.dark_mode,
    });

    // ── Conectar callbacks ────────────────────────────────────────────────

    // Descarga de modelos
    {
        let w = window.as_weak();
        let p = app_paths.clone();
        window.on_start_download(move || {
            downloader::iniciar_descarga(w.clone(), p.clone());
        });
    }
    {
        let w = window.as_weak();
        let p = app_paths.clone();
        window.on_retry_download(move || {
            downloader::iniciar_descarga(w.clone(), p.clone());
        });
    }

    // Seleccionar archivo
    {
        let s = state.clone();
        window.on_select_file(move || {
            s.abrir_file_picker();
        });
    }

    // Drop de archivo (drag & drop)
    {
        let s = state.clone();
        window.on_drop_file(move |path| {
            s.aplicar_archivo_seleccionado(Some(path.to_string()));
        });
    }

    // Iniciar procesamiento
    {
        let s = state.clone();
        let w = window.as_weak();
        let manifest = app_paths.manifest.to_string_lossy().to_string();
        window.on_start_processing(move || {
            let Some(win) = w.upgrade() else { return };
            let Some(file) = s.selected_file() else { return };
            let output_dir = win.get_output_dir().to_string();
            let cfg = s.config();
            processor::iniciar_procesamiento(
                w.clone(),
                file,
                output_dir,
                cfg,
                manifest.clone(),
            );
        });
    }

    // Abrir carpeta de salida
    {
        let w = window.as_weak();
        window.on_open_output_folder(move || {
            let Some(win) = w.upgrade() else { return };
            let path = win.get_output_path().to_string();
            if !path.is_empty() {
                let _ = open::that(path);
            }
        });
    }

    // Guardar configuración
    {
        let s = state.clone();
        let w = window.as_weak();
        let p = app_paths.clone();
        window.on_save_settings(move |settings_data| {
            let mut cfg = s.config();
            cfg.pipeline.ocr.confidence_threshold = settings_data.confidence_threshold;
            cfg.pipeline.input.rasterization_dpi = settings_data.dpi as u32;
            cfg.output_dir = settings_data.output_dir.to_string();
            cfg.dark_mode = settings_data.dark_mode;

            s.set_config(cfg.clone());
            let _ = cfg.guardar(&p);

            let new_settings = settings_data.clone();
            if let Some(win) = w.upgrade() {
                win.set_dark_mode(settings_data.dark_mode);
                win.set_current_settings(new_settings);
                win.set_output_dir(settings_data.output_dir);
            }
        });
    }

    // Navegar a página
    {
        let s = state.clone();
        window.on_navigate_to(move |page| {
            s.navegar_a(page);
        });
    }

    // ── Verificar modelos al inicio ───────────────────────────────────────
    model_checker::verificar_y_poblar(window.as_weak(), app_paths);

    // Bloquea hasta que el usuario cierre la ventana
    window.run()?;

    Ok(())
}
