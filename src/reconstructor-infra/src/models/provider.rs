// =============================================================================
// HuggingFaceModelProvider — Ingesta HTTP de pesos neuronales
//
// Propósito: Concreción de la abstracción `ModelProvider` para operar contra 
//            un registro remoto público. Resuelve el requerimiento "plug-and-play"
//            hidratando localmente los tensores ausentes sin scripts externos.
// =============================================================================

use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};
use reconstructor_domain::traits::{DownloadProgress, ModelEntry, ModelProvider, ModelStatus};
use reconstructor_domain::DomainError;

use crate::error::InfraError;

/// Conector síncrono al CDN estático de HuggingFace.
pub struct HuggingFaceModelProvider;

impl ModelProvider for HuggingFaceModelProvider {
    fn verificar_modelos(&self, entries: &[ModelEntry]) -> Vec<(String, ModelStatus)> {
        entries
            .iter()
            .map(|e| {
                let status = verificar_entrada(e);
                (e.id.clone(), status)
            })
            .collect()
    }

    /// Hidrata un blob pre-compilado asumiendo conectividad HTTPS.
    ///
    /// Bloquea el hilo actual pero permite comunicación paralela con la UI 
    /// a través del Event Bus (`on_progress`).
    ///
    /// # Arguments
    ///
    /// * `entry` - Descriptor estático del manifiesto a recuperar.
    /// * `on_progress` - Closure de despacho de telemetría.
    ///
    /// # Errors
    ///
    /// Retorna `DomainError` (InfraError) si la capa de sockets TCP falla,
    /// se obtiene un código HTTP de error, o si la función de hash criptográfico 
    /// revela manipulación/corrupción (Man-in-the-Middle o disco lleno).
    fn descargar_modelo(
        &self,
        entry: &ModelEntry,
        on_progress: Box<dyn Fn(DownloadProgress) + Send>,
    ) -> Result<(), DomainError> {
        let url = format!(
            "https://huggingface.co/{}/resolve/main/{}",
            entry.repo, entry.path_repo
        );

        tracing::info!("Descargando {} desde {}", entry.id, url);

        let client = reqwest::blocking::Client::new();
        let mut response = client
            .get(&url)
            .send()
            .map_err(|e| InfraError::Red(e.to_string()))?;

        if !response.status().is_success() {
            return Err(InfraError::Red(format!(
                "HTTP {} al descargar {}",
                response.status(),
                url
            ))
            .into());
        }

        let total_bytes = response.content_length().unwrap_or(0);

        if let Some(parent) = Path::new(&entry.path_local).parent() {
            fs::create_dir_all(parent).map_err(InfraError::Io)?;
        }

        let mut archivo =
            fs::File::create(&entry.path_local).map_err(InfraError::Io)?;

        let mut buf = [0u8; 65536];
        let mut descargados = 0u64;

        loop {
            let n = response.read(&mut buf).map_err(InfraError::Io)?;
            if n == 0 {
                break;
            }
            archivo.write_all(&buf[..n]).map_err(InfraError::Io)?;
            descargados += n as u64;
            on_progress(DownloadProgress {
                model_id: entry.id.clone(),
                bytes_descargados: descargados,
                bytes_totales: total_bytes,
            });
        }

        tracing::info!("Descarga completa: {} ({} bytes)", entry.id, descargados);

        if !entry.sha256.is_empty() {
            let ok = self
                .verificar_integridad(&entry.path_local, &entry.sha256)?;
            if !ok {
                let obtenido = sha256_de_archivo(&entry.path_local)
                    .map_err(InfraError::Io)?;
                return Err(InfraError::HashIncorrecto {
                    esperado: entry.sha256.clone(),
                    obtenido,
                }
                .into());
            }
        }

        Ok(())
    }

    fn verificar_integridad(
        &self,
        path_local: &str,
        sha256_esperado: &str,
    ) -> Result<bool, DomainError> {
        if sha256_esperado.is_empty() {
            return Ok(true);
        }
        let hash = sha256_de_archivo(path_local).map_err(InfraError::Io)?;
        Ok(hash == sha256_esperado)
    }
}

fn verificar_entrada(entry: &ModelEntry) -> ModelStatus {
    let path = Path::new(&entry.path_local);
    if !path.exists() {
        return ModelStatus::Faltante;
    }
    if entry.sha256.is_empty() {
        return ModelStatus::Ok;
    }
    match sha256_de_archivo(&entry.path_local) {
        Ok(hash) if hash == entry.sha256 => ModelStatus::Ok,
        Ok(_) => ModelStatus::Corrupto,
        Err(_) => ModelStatus::Corrupto,
    }
}

fn sha256_de_archivo(path: &str) -> Result<String, std::io::Error> {
    let mut archivo = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = archivo.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let result = hasher.finalize();
    Ok(result.iter().map(|b| format!("{:02x}", b)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn entry_test(path_local: &str, sha256: &str) -> ModelEntry {
        ModelEntry {
            id: "test_model".to_string(),
            nombre: "Test Model".to_string(),
            repo: "owner/repo".to_string(),
            path_repo: "model.onnx".to_string(),
            path_local: path_local.to_string(),
            sha256: sha256.to_string(),
            size_mb: 1,
        }
    }

    #[test]
    fn verificar_modelo_faltante() {
        let provider = HuggingFaceModelProvider;
        let entry = entry_test("/ruta/que/no/existe.onnx", "");
        let resultados = provider.verificar_modelos(&[entry]);
        assert_eq!(resultados[0].1, ModelStatus::Faltante);
    }

    #[test]
    fn verificar_modelo_existente_sin_hash() {
        let provider = HuggingFaceModelProvider;
        let tmp_path = std::env::temp_dir().join("test_modelo.onnx");
        let mut f = fs::File::create(&tmp_path).unwrap();
        f.write_all(b"fake model data").unwrap();

        let entry = entry_test(tmp_path.to_str().unwrap(), "");
        let resultados = provider.verificar_modelos(&[entry]);
        assert_eq!(resultados[0].1, ModelStatus::Ok);

        let _ = fs::remove_file(&tmp_path);
    }

    #[test]
    fn verificar_integridad_hash_correcto() {
        let provider = HuggingFaceModelProvider;
        let tmp_path = std::env::temp_dir().join("test_hash.bin");
        let contenido = b"hola mundo";
        fs::write(&tmp_path, contenido).unwrap();

        let hash_esperado = sha256_de_archivo(tmp_path.to_str().unwrap()).unwrap();
        let ok = provider
            .verificar_integridad(tmp_path.to_str().unwrap(), &hash_esperado)
            .unwrap();
        assert!(ok);

        let _ = fs::remove_file(&tmp_path);
    }

    #[test]
    fn verificar_integridad_hash_incorrecto() {
        let provider = HuggingFaceModelProvider;
        let tmp_path = std::env::temp_dir().join("test_hash_mal.bin");
        fs::write(&tmp_path, b"datos reales").unwrap();

        let ok = provider
            .verificar_integridad(tmp_path.to_str().unwrap(), "hashfalsoxyz")
            .unwrap();
        assert!(!ok);

        let _ = fs::remove_file(&tmp_path);
    }

    #[test]
    #[ignore]
    fn descargar_modelo_real() {
        let provider = HuggingFaceModelProvider;
        let tmp_path = std::env::temp_dir().join("modelo_descargado.onnx");
        let entry = ModelEntry {
            id: "test".to_string(),
            nombre: "Test".to_string(),
            repo: "hf-internal-testing/tiny-random-bert".to_string(),
            path_repo: "README.md".to_string(),
            path_local: tmp_path.to_str().unwrap().to_string(),
            sha256: "".to_string(),
            size_mb: 0,
        };
        provider
            .descargar_modelo(&entry, Box::new(|_| {}))
            .unwrap();
        assert!(tmp_path.exists());
        let _ = fs::remove_file(&tmp_path);
    }
}
