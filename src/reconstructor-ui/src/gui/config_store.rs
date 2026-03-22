// =============================================================================
// config_store — Persistencia de configuración del usuario
//
// Guarda UserConfig en <config_dir>/reconstructor/user.toml (vía AppPaths).
// Fusiona con PipelineConfig del dominio para que compose.rs lo pueda usar.
// =============================================================================

use anyhow::{Context, Result};
use reconstructor_domain::PipelineConfig;
use serde::{Deserialize, Serialize};

use crate::paths::AppPaths;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserConfig {
    #[serde(flatten)]
    pub pipeline: PipelineConfig,
    pub output_dir: String,
    pub dark_mode: bool,
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            pipeline: PipelineConfig::default(),
            output_dir: dirs::download_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .to_string_lossy()
                .to_string(),
            dark_mode: false,
        }
    }
}

impl UserConfig {
    /// Carga la config del usuario usando los paths resueltos por AppPaths.
    /// Devuelve Default si el archivo no existe o es inválido.
    pub fn cargar(paths: &AppPaths) -> Self {
        Self::intentar_cargar(paths).unwrap_or_else(|_| Self::default())
    }

    fn intentar_cargar(paths: &AppPaths) -> Result<Self> {
        let contenido = std::fs::read_to_string(&paths.user_config)
            .with_context(|| format!("No se pudo leer {:?}", paths.user_config))?;
        toml::from_str(&contenido).context("UserConfig TOML inválida")
    }

    pub fn guardar(&self, paths: &AppPaths) -> Result<()> {
        std::fs::create_dir_all(&paths.config_dir)
            .with_context(|| format!("No se pudo crear {:?}", paths.config_dir))?;
        let contenido = toml::to_string_pretty(self).context("Error serializando UserConfig")?;
        std::fs::write(&paths.user_config, contenido)
            .with_context(|| format!("No se pudo escribir {:?}", paths.user_config))
    }
}
