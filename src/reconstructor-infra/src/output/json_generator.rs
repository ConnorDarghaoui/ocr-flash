// =============================================================================
// JsonOutputGenerator — Serialización del Árbol de Auditoría
//
// Propósito: Generar el "Receipt" o traza de ejecución inmutable requerida para 
//            validación de QA y trazabilidad del sistema (F4.5).
//            Se asegura de que el historial de la Máquina de Estados se proyecte
//            fielmente a un formato de consumo universal.
// =============================================================================

use std::fs;

use reconstructor_domain::traits::{ComposedPage, OutputGenerator};
use reconstructor_domain::{Document, DomainError};

use crate::error::InfraError;

/// Motor de inyección I/O para trazas estructuradas.
pub struct JsonOutputGenerator;

impl OutputGenerator for JsonOutputGenerator {
    /// Despliega el estado acumulado del Orquestador a almacenamiento secundario.
    ///
    /// # Arguments
    ///
    /// * `documento` - Estado canónico completo incluyendo métricas de inferencia e historiales FSM.
    /// * `_paginas` - Vectores renderizados (ignorados, ya que este adaptador es puramente semántico).
    /// * `ruta_salida` - Prefijo canónico sin extensión garantizado por el llamador.
    ///
    /// # Errors
    ///
    /// Lanza un error encapsulado del SO (`DomainError`) si el serializador agota el heap de la 
    /// aplicación o si los permisos de I/O deniegan la escritura.
    fn generar(
        &self,
        documento: &Document,
        _paginas: &[ComposedPage],
        ruta_salida: &str,
    ) -> Result<(), DomainError> {
        let json = serde_json::to_string_pretty(documento)
            .map_err(|e| InfraError::Io(std::io::Error::other(e.to_string())))?;
        let ruta = format!("{ruta_salida}.json");
        fs::write(&ruta, json).map_err(InfraError::Io)?;
        tracing::info!("JSON escrito en {ruta}");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reconstructor_domain::{Document, ProcessingMetrics};
    use std::fs;

    fn documento_minimo() -> Document {
        Document {
            ruta_origen: "test.png".to_string(),
            version_pipeline: "0.1.0".to_string(),
            procesado_en: "2024-01-01T00:00:00Z".to_string(),
            paginas: vec![],
            metricas: ProcessingMetrics::default(),
        }
    }

    #[test]
    fn genera_json_valido() {
        let gen = JsonOutputGenerator;
        let doc = documento_minimo();
        let ruta_base = std::env::temp_dir().join("test_json_gen");
        let ruta_str = ruta_base.to_str().unwrap();

        gen.generar(&doc, &[], ruta_str).unwrap();

        let ruta_json = format!("{ruta_str}.json");
        let contenido = fs::read_to_string(&ruta_json).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&contenido).unwrap();
        assert_eq!(parsed["ruta_origen"], "test.png");
        let _ = fs::remove_file(&ruta_json);
    }
}
