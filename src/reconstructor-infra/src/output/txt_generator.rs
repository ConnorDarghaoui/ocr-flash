// =============================================================================
// TxtOutputGenerator — Extracción de Texto Plano Consolidado
//
// Propósito: Interfaz de generación diseñada para proveer una copia *stripped-down*
//            ideal para indexación masiva por motores de búsqueda empresariales 
//            o inyección en flujos NLP (Natural Language Processing) de terceros.
// =============================================================================

use std::fs;

use reconstructor_domain::traits::{ComposedPage, OutputGenerator};
use reconstructor_domain::{Document, DomainError};

use crate::error::InfraError;

/// Motor de inyección I/O para texto plano.
pub struct TxtOutputGenerator;

impl OutputGenerator for TxtOutputGenerator {
    /// Despliega el contenido semántico textual del grafo ignorando entidades espaciales y rasterizadas.
    ///
    /// # Arguments
    ///
    /// * `_documento` - Estado canónico ignorado por este exportador.
    /// * `paginas` - Buffers independientes que contienen los strings pre-compilados.
    /// * `ruta_salida` - Prefijo canónico sin extensión garantizado por el llamador.
    ///
    /// # Errors
    ///
    /// Retorna `DomainError` ante fallos en los descriptores de archivo del Host (Ej. I/O permission denied).
    fn generar(
        &self,
        _documento: &Document,
        paginas: &[ComposedPage],
        ruta_salida: &str,
    ) -> Result<(), DomainError> {
        let mut contenido = String::new();
        for pagina in paginas {
            if !contenido.is_empty() {
                contenido.push('\n');
            }
            contenido.push_str(&format!("--- Página {} ---\n", pagina.numero_pagina));
            contenido.push_str(&pagina.texto_extraido);
        }
        let ruta = format!("{ruta_salida}.txt");
        fs::write(&ruta, &contenido).map_err(InfraError::Io)?;
        tracing::info!("TXT escrito en {ruta}");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reconstructor_domain::{Document, ProcessingMetrics};
    use reconstructor_domain::traits::ComposedPage;
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

    fn pagina(n: u32, texto: &str) -> ComposedPage {
        ComposedPage {
            numero_pagina: n,
            pdf_bytes: vec![],
            texto_extraido: texto.to_string(),
        }
    }

    #[test]
    fn genera_txt_con_separadores() {
        let gen = TxtOutputGenerator;
        let doc = documento_minimo();
        let paginas = vec![pagina(1, "Hola mundo"), pagina(2, "Segunda página")];
        let ruta_base = std::env::temp_dir().join("test_txt_gen");
        let ruta_str = ruta_base.to_str().unwrap();

        gen.generar(&doc, &paginas, ruta_str).unwrap();

        let ruta_txt = format!("{ruta_str}.txt");
        let contenido = fs::read_to_string(&ruta_txt).unwrap();
        assert!(contenido.contains("--- Página 1 ---"));
        assert!(contenido.contains("Hola mundo"));
        assert!(contenido.contains("--- Página 2 ---"));
        assert!(contenido.contains("Segunda página"));
        let _ = fs::remove_file(&ruta_txt);
    }

    #[test]
    fn genera_txt_vacio_sin_paginas() {
        let gen = TxtOutputGenerator;
        let doc = documento_minimo();
        let ruta_base = std::env::temp_dir().join("test_txt_vacio");
        let ruta_str = ruta_base.to_str().unwrap();

        gen.generar(&doc, &[], ruta_str).unwrap();

        let ruta_txt = format!("{ruta_str}.txt");
        let contenido = fs::read_to_string(&ruta_txt).unwrap();
        assert!(contenido.is_empty());
        let _ = fs::remove_file(&ruta_txt);
    }
}
