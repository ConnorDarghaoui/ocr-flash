// =============================================================================
// PdfOutputGenerator — Serialización de Buffer Vectorial (PDF)
//
// Propósito: Desacoplar el dominio del file-system. Actúa como el sumidero final
//            del pipeline inyectando el resultado renderizado a disco. Implementa
//            una política de segmentación forzada por página si el documento original 
//            es compuesto, evitando unificar páginas discordantes.
// =============================================================================

use std::fs;

use reconstructor_domain::traits::{ComposedPage, OutputGenerator};
use reconstructor_domain::{Document, DomainError};

use crate::error::InfraError;

/// Motor de inyección I/O para artefactos PDF.
pub struct PdfOutputGenerator;

impl OutputGenerator for PdfOutputGenerator {
    /// Vacía los buffers binarios al sistema de almacenamiento persistente.
    ///
    /// Transmuta silenciosamente un único documento lógico en múltiples ficheros físicos
    /// (`{ruta_salida}_p{N}.pdf`) si la entrada sobrepasa una página para prevenir
    /// la corrupción de la metadata nativa PDF.
    ///
    /// # Arguments
    ///
    /// * `_documento` - Estado canónico ignorado por este exportador.
    /// * `paginas` - Buffers independientes listos para su inyección binaria.
    /// * `ruta_salida` - Prefijo canónico sin extensión garantizado por el llamador.
    ///
    /// # Errors
    ///
    /// Retorna `DomainError` ante bloqueos de escritura a nivel de SO (File Lock, 
    /// permisos insuficientes, o No Space Left on Device).
    fn generar(
        &self,
        _documento: &Document,
        paginas: &[ComposedPage],
        ruta_salida: &str,
    ) -> Result<(), DomainError> {
        if paginas.is_empty() {
            return Ok(());
        }
        if paginas.len() == 1 {
            let ruta = format!("{ruta_salida}.pdf");
            fs::write(&ruta, &paginas[0].pdf_bytes).map_err(InfraError::Io)?;
            tracing::info!("PDF escrito en {ruta}");
        } else {
            for pagina in paginas {
                let ruta = format!("{ruta_salida}_p{}.pdf", pagina.numero_pagina);
                fs::write(&ruta, &pagina.pdf_bytes).map_err(InfraError::Io)?;
                tracing::info!("PDF escrito en {ruta}");
            }
        }
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

    #[test]
    fn una_pagina_genera_archivo_unico() {
        let gen = PdfOutputGenerator;
        let doc = documento_minimo();
        let paginas = vec![ComposedPage {
            numero_pagina: 1,
            pdf_bytes: b"fake-pdf".to_vec(),
            texto_extraido: String::new(),
        }];
        let ruta_base = std::env::temp_dir().join("test_pdf_gen_single");
        let ruta_str = ruta_base.to_str().unwrap();

        gen.generar(&doc, &paginas, ruta_str).unwrap();

        let ruta_pdf = format!("{ruta_str}.pdf");
        assert!(fs::metadata(&ruta_pdf).is_ok());
        let _ = fs::remove_file(&ruta_pdf);
    }

    #[test]
    fn multiples_paginas_generan_archivos_por_pagina() {
        let gen = PdfOutputGenerator;
        let doc = documento_minimo();
        let paginas = vec![
            ComposedPage { numero_pagina: 1, pdf_bytes: b"p1".to_vec(), texto_extraido: String::new() },
            ComposedPage { numero_pagina: 2, pdf_bytes: b"p2".to_vec(), texto_extraido: String::new() },
        ];
        let ruta_base = std::env::temp_dir().join("test_pdf_gen_multi");
        let ruta_str = ruta_base.to_str().unwrap();

        gen.generar(&doc, &paginas, ruta_str).unwrap();

        assert!(fs::metadata(format!("{ruta_str}_p1.pdf")).is_ok());
        assert!(fs::metadata(format!("{ruta_str}_p2.pdf")).is_ok());
        let _ = fs::remove_file(format!("{ruta_str}_p1.pdf"));
        let _ = fs::remove_file(format!("{ruta_str}_p2.pdf"));
    }
}
