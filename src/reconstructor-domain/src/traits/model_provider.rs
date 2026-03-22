// =============================================================================
// ModelProvider — Abstracción de provisión de dependencias (Modelos ONNX)
//
// Propósito: Desacopla la lógica de verificación y descarga de modelos de las 
//            fuentes externas (HuggingFace, red corporativa) aislando al dominio 
//            según el ADR-010.
// =============================================================================

use crate::error::DomainError;

/// Declaración inmutable de un artefacto requerido leída desde el manifiesto.
#[derive(Debug, Clone)]
pub struct ModelEntry {
    pub id: String,
    pub nombre: String,
    pub repo: String,
    pub path_repo: String,
    pub path_local: String,
    pub sha256: String,
    pub size_mb: u32,
}

#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub model_id: String,
    pub bytes_descargados: u64,
    pub bytes_totales: u64,
}

impl DownloadProgress {
    pub fn fraccion(&self) -> f32 {
        if self.bytes_totales == 0 {
            return 0.0;
        }
        (self.bytes_descargados as f32 / self.bytes_totales as f32).min(1.0)
    }
}

/// Garantiza la ejecución predecible previniendo arranques con modelos adulterados.
#[derive(Debug, Clone, PartialEq)]
pub enum ModelStatus {
    Ok,
    Faltante,
    Corrupto,
}

/// Contrato para la inyección de la lógica de hidratación de modelos.
///
/// La existencia de este Trait previene que la máquina de estados se acople 
/// directamente a un cliente HTTP o al sistema de archivos local.
pub trait ModelProvider: Send + Sync {
    /// Determina el estado del entorno verificando existencia y cifrado de los archivos.
    ///
    /// # Arguments
    ///
    /// * `entries` - Array estático de requisitos extraídos del manifiesto (toml).
    fn verificar_modelos(&self, entries: &[ModelEntry]) -> Vec<(String, ModelStatus)>;

    /// Inicia la recuperación del archivo faltante de manera asíncrona pero bloqueando
    /// el paso de la máquina de estados hasta finalizar.
    ///
    /// # Arguments
    ///
    /// * `entry` - Descriptor del modelo a hidratar.
    /// * `on_progress` - Callback (Closure) inyectado para despachar métricas al event bus.
    fn descargar_modelo(
        &self,
        entry: &ModelEntry,
        on_progress: Box<dyn Fn(DownloadProgress) + Send>,
    ) -> Result<(), DomainError>;

    /// Valida matemáticamente que el artefacto descargado sea auténtico mediante hashing.
    fn verificar_integridad(&self, path_local: &str, sha256_esperado: &str) -> Result<bool, DomainError>;
}
