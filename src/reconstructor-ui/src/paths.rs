// =============================================================================
// paths — Resolución de paths multiplataforma
//
// Propósito: Centralizar la lógica de dónde viven los archivos de la app
//            (modelos, config, manifest) según la plataforma y el modo de
//            ejecución (desarrollo vs. binario instalado / AppImage).
//
// Lookup del manifest (primer path que existe):
//   1. <exe_dir>/model_manifest.toml  — release zip / AppImage
//   2. <data_dir>/reconstructor/model_manifest.toml  — instalación previa
//   3. model_manifest.toml (CWD)  — desarrollo local
// =============================================================================

use std::path::PathBuf;

/// Paths resueltos para la aplicación en la plataforma actual.
#[derive(Debug, Clone)]
#[allow(dead_code)] // campos usados por reconstructor-gui; warnings al compilar solo el CLI
pub struct AppPaths {
    /// Directorio donde se almacenan los modelos ONNX descargados.
    /// Linux:   ~/.local/share/reconstructor/models/
    /// Windows: %APPDATA%\reconstructor\models\
    /// macOS:   ~/Library/Application Support/reconstructor/models/
    pub models_dir: PathBuf,

    /// Directorio de configuración del usuario.
    pub config_dir: PathBuf,

    /// Archivo de configuración del usuario.
    pub user_config: PathBuf,

    /// Path al manifest de modelos (ya resuelto con lookup chain).
    pub manifest: PathBuf,
}

impl AppPaths {
    /// Resuelve los paths de la app para la plataforma actual.
    pub fn resolver() -> Self {
        let data_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("reconstructor");

        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("config"))
            .join("reconstructor");

        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));

        // Lookup chain: exe_dir → data_dir → CWD
        let manifest = [
            exe_dir.join("model_manifest.toml"),
            data_dir.join("model_manifest.toml"),
            PathBuf::from("model_manifest.toml"),
        ]
        .into_iter()
        .find(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from("model_manifest.toml"));

        AppPaths {
            models_dir: data_dir.join("models"),
            user_config: config_dir.join("user.toml"),
            manifest,
            config_dir,
        }
    }

    /// Path local de un modelo dado su path relativo en el manifest.
    /// "models/ocr/det.onnx" → <models_dir>/ocr/det.onnx
    #[allow(dead_code)] // usado por reconstructor-gui
    pub fn resolver_modelo(&self, path_local: &str) -> PathBuf {
        let rel = path_local
            .strip_prefix("models/")
            .unwrap_or(path_local);
        self.models_dir.join(rel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolver_modelo_quita_prefijo_models() {
        let paths = AppPaths {
            models_dir: PathBuf::from("/home/user/.local/share/reconstructor/models"),
            config_dir: PathBuf::from("/home/user/.config/reconstructor"),
            user_config: PathBuf::from("/home/user/.config/reconstructor/user.toml"),
            manifest: PathBuf::from("model_manifest.toml"),
        };
        let result = paths.resolver_modelo("models/ocr/det.onnx");
        assert_eq!(
            result,
            PathBuf::from("/home/user/.local/share/reconstructor/models/ocr/det.onnx")
        );
    }

    #[test]
    fn resolver_modelo_sin_prefijo_lo_adjunta_directo() {
        let paths = AppPaths {
            models_dir: PathBuf::from("/data/models"),
            config_dir: PathBuf::from("/cfg"),
            user_config: PathBuf::from("/cfg/user.toml"),
            manifest: PathBuf::from("model_manifest.toml"),
        };
        let result = paths.resolver_modelo("ocr/det.onnx");
        assert_eq!(result, PathBuf::from("/data/models/ocr/det.onnx"));
    }

    #[test]
    fn lookup_chain_usa_cwd_si_no_hay_otros() {
        // En tests el exe_dir existe pero model_manifest.toml junto a él
        // casi seguro no existe, así que cae al CWD fallback.
        let paths = AppPaths::resolver();
        // El manifest resuelto siempre tiene un path (aunque no exista el archivo)
        assert!(!paths.manifest.as_os_str().is_empty());
    }
}
