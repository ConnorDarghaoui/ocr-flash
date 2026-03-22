// =============================================================================
// ModelManifest — Cargador de Manifiesto de Infraestructura
//
// Propósito: Desacopla la configuración de red y almacenamiento local (Hardcoded URIs)
//            del proceso de bootstraping. Provee el "Source of Truth" de las dependencias ML
//            requeridas para transicionar el FSM de `CheckingModels` a `Ready`.
// =============================================================================

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use reconstructor_domain::traits::ModelEntry;

use crate::error::InfraError;

#[derive(Debug, Deserialize)]
struct ModelEntryToml {
    #[serde(rename = "name")]
    nombre: String,
    repo: String,
    #[serde(rename = "path")]
    path_repo: String,
    local_path: String,
    #[serde(default)]
    sha256: String,
    #[serde(default)]
    size_mb: u32,
}

#[derive(Debug, Deserialize)]
struct ManifestFile {
    models: HashMap<String, ModelEntryToml>,
}

/// Aísla la capa de persistencia (TOML) traduciendo sus nodos nativos a entidades inmutables del Dominio (`ModelEntry`).
pub struct ModelManifest;

impl ModelManifest {
    pub fn cargar(path: &str) -> Result<Vec<ModelEntry>, InfraError> {
        let contenido = std::fs::read_to_string(path).map_err(InfraError::Io)?;
        Self::parsear(&contenido)
    }

    /// Resuelve las rutas abstractas (`models/...`) contra el path dinámico del entorno (`AppPaths`).
    ///
    /// Garantiza que en entornos sandboxed (Flatpak, AppImage) la aplicación pueda escribir los 
    /// tensores descargados en su carpeta local de usuario en vez de fallar por permisos en el binario raíz.
    ///
    /// # Arguments
    ///
    /// * `manifest_path` - Fichero base de definición estructural.
    /// * `models_dir` - Directorio destino calculado por el host.
    pub fn cargar_con_base_dir(
        manifest_path: &std::path::Path,
        models_dir: &std::path::Path,
    ) -> Result<Vec<ModelEntry>, InfraError> {
        let entries = Self::cargar(manifest_path.to_str().unwrap_or(""))?;
        Ok(entries
            .into_iter()
            .map(|mut e| {
                let rel = e
                    .path_local
                    .strip_prefix("models/")
                    .unwrap_or(&e.path_local)
                    .to_string();
                e.path_local = models_dir.join(rel).to_string_lossy().to_string();
                e
            })
            .collect())
    }

    pub fn calcular_sha256(path: &Path) -> Result<String, InfraError> {
        let mut archivo = std::fs::File::open(path).map_err(InfraError::Io)?;
        let mut hasher = Sha256::new();
        let mut buf = [0u8; 65536];
        loop {
            let n = archivo.read(&mut buf).map_err(InfraError::Io)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        let result = hasher.finalize();
        Ok(result.iter().map(|b| format!("{:02x}", b)).collect())
    }

    pub fn parsear(toml_str: &str) -> Result<Vec<ModelEntry>, InfraError> {
        let manifest: ManifestFile = toml::from_str(toml_str)
            .map_err(|e| InfraError::Io(std::io::Error::other(e.to_string())))?;

        let mut entries: Vec<ModelEntry> = manifest
            .models
            .into_iter()
            .map(|(id, e)| ModelEntry {
                id,
                nombre: e.nombre,
                repo: e.repo,
                path_repo: e.path_repo,
                path_local: e.local_path,
                sha256: e.sha256,
                size_mb: e.size_mb,
            })
            .collect();

        entries.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST_MINIMO: &str = r#"
[models.ocr_det]
name = "PP-OCRv5 Detection"
repo = "monkt/paddleocr-onnx"
path = "detection/v5/det.onnx"
local_path = "models/ocr/det.onnx"
sha256 = "abc123"
size_mb = 84

[models.layout]
name = "DocLayout-YOLO"
repo = "wybxc/DocLayout-YOLO-DocStructBench-onnx"
path = "doclayout_yolo_docstructbench_imgsz1024.onnx"
local_path = "models/layout/doclayout_yolo.onnx"
sha256 = ""
size_mb = 75
"#;

    #[test]
    fn parsea_manifest_correctamente() {
        let entries = ModelManifest::parsear(MANIFEST_MINIMO).unwrap();
        assert_eq!(entries.len(), 2);

        let det = entries.iter().find(|e| e.id == "ocr_det").unwrap();
        assert_eq!(det.nombre, "PP-OCRv5 Detection");
        assert_eq!(det.repo, "monkt/paddleocr-onnx");
        assert_eq!(det.path_repo, "detection/v5/det.onnx");
        assert_eq!(det.path_local, "models/ocr/det.onnx");
        assert_eq!(det.sha256, "abc123");
        assert_eq!(det.size_mb, 84);
    }

    #[test]
    fn parsea_sha256_vacio() {
        let entries = ModelManifest::parsear(MANIFEST_MINIMO).unwrap();
        let layout = entries.iter().find(|e| e.id == "layout").unwrap();
        assert!(layout.sha256.is_empty());
    }

    #[test]
    fn toml_invalido_retorna_error() {
        let result = ModelManifest::parsear("esto no es toml válido {{{{");
        assert!(result.is_err());
    }

    #[test]
    fn cargar_con_base_dir_resuelve_paths_relativos() {
        use std::io::Write;
        let manifest_path = std::env::temp_dir().join("test_manifest_base_dir.toml");
        let mut f = std::fs::File::create(&manifest_path).unwrap();
        writeln!(f, "{MANIFEST_MINIMO}").unwrap();

        let models_dir = std::path::Path::new("/data/models");
        let entries =
            ModelManifest::cargar_con_base_dir(&manifest_path, models_dir).unwrap();

        let det = entries.iter().find(|e| e.id == "ocr_det").unwrap();
        assert_eq!(det.path_local, "/data/models/ocr/det.onnx");

        let layout = entries.iter().find(|e| e.id == "layout").unwrap();
        assert_eq!(layout.path_local, "/data/models/layout/doclayout_yolo.onnx");
    }
}
