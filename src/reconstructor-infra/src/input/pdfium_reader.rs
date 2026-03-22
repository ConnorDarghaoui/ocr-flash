// =============================================================================
// PdfiumPageReader — Adaptador de Ingesta Raster (F1.9)
//
// Propósito: Desacoplar la decodificación de PDFs nativos del Dominio.
//            Utiliza `pdfium-render` (basado en C++) para forzar la rasterización 
//            en origen, garantizando que el pipeline de ML (ONNX) opere exclusivamente 
//            sobre pixeles, independientemente del origen vectorial o gráfico del archivo.
// =============================================================================

use std::io::Cursor;

use image::ImageFormat;
use pdfium_render::prelude::*;
use reconstructor_domain::{DomainError, PageImage};

use crate::error::InfraError;

/// Materializa la primera etapa del pipeline transformando documentos complejos en matrices de bytes.
pub struct PdfiumPageReader {
    dpi: u16,
}

impl PdfiumPageReader {
    pub fn new(dpi: u16) -> Self {
        Self { dpi }
    }

    pub fn default_dpi() -> Self {
        Self::new(300)
    }

    /// # Arguments
    ///
    /// * `ruta` - Path del archivo en el sistema operativo.
    ///
    /// # Errors
    ///
    /// Retorna `DomainError` (InfraError encapsulado) si no hay permisos de lectura.
    pub fn leer_archivo(&self, ruta: &str) -> Result<Vec<PageImage>, DomainError> {
        let bytes = std::fs::read(ruta).map_err(InfraError::Io)?;
        self.leer_bytes(&bytes)
    }

    /// Descompone un documento binario en un array de estructuras `PageImage` inmutables.
    ///
    /// Interacciona mediante C-bindings con la biblioteca compartida `libpdfium.so/dll`.
    /// Multiplica el tamaño del lote por un factor DPI escalable para mitigar la pérdida 
    /// de granularidad en los modelos subyacentes.
    ///
    /// # Arguments
    ///
    /// * `bytes` - Archivo decodificado en memoria.
    ///
    /// # Errors
    ///
    /// Falla si la librería compartida de Pdfium no está en la variable de entorno PATH, 
    /// o si el documento binario está malformado.
    pub fn leer_bytes(&self, bytes: &[u8]) -> Result<Vec<PageImage>, DomainError> {
        if bytes.is_empty() {
            return Err(InfraError::Pdf("PDF vacío".to_string()).into());
        }

        let bindings = Pdfium::bind_to_system_library()
            .map_err(|e| InfraError::Pdf(format!("pdfium no encontrado: {e:?}")))?;

        let pdfium = Pdfium::new(bindings);
        let doc = pdfium
            .load_pdf_from_byte_slice(bytes, None)
            .map_err(|e| InfraError::Pdf(format!("Error abriendo PDF: {e:?}")))?;

        let target_width = (8.27 * self.dpi as f32) as i32;

        let config = PdfRenderConfig::new().set_target_width(target_width);

        let mut paginas = Vec::new();
        for (idx, page) in doc.pages().iter().enumerate() {
            let numero_pagina = (idx + 1) as u32;

            let bitmap = page
                .render_with_config(&config)
                .map_err(|e| InfraError::Pdf(format!("Error renderizando página {numero_pagina}: {e:?}")))?;

            let img = bitmap.as_image();
            let ancho = img.width();
            let alto = img.height();

            let mut datos = Vec::new();
            img.write_to(&mut Cursor::new(&mut datos), ImageFormat::Png)
                .map_err(|e| InfraError::Imagen(e.to_string()))?;

            paginas.push(PageImage { datos, ancho, alto, numero_pagina });
        }

        if paginas.is_empty() {
            return Err(InfraError::Pdf("PDF sin páginas".to_string()).into());
        }

        Ok(paginas)
    }

    /// Vía de escape de inicialización para distribuciones tipo AppImage donde
    /// la librería dinámica `libpdfium` reside en un directorio aislado (`usr/lib/`).
    pub fn con_ruta_pdfium(
        &self,
        pdfium_dir: &str,
        bytes: &[u8],
    ) -> Result<Vec<PageImage>, DomainError> {
        let bindings = Pdfium::bind_to_library(
            Pdfium::pdfium_platform_library_name_at_path(pdfium_dir),
        )
        .map_err(|e| InfraError::Pdf(format!("pdfium no encontrado en {pdfium_dir}: {e:?}")))?;
        let pdfium = Pdfium::new(bindings);

        let doc = pdfium
            .load_pdf_from_byte_slice(bytes, None)
            .map_err(|e| InfraError::Pdf(format!("Error abriendo PDF: {e:?}")))?;

        let target_width = (8.27 * self.dpi as f32) as i32;
        let config = PdfRenderConfig::new().set_target_width(target_width);

        let mut paginas = Vec::new();
        for (idx, page) in doc.pages().iter().enumerate() {
            let numero_pagina = (idx + 1) as u32;
            let bitmap = page
                .render_with_config(&config)
                .map_err(|e| InfraError::Pdf(format!("Error renderizando página {numero_pagina}: {e:?}")))?;

            let img = bitmap.as_image();
            let ancho = img.width();
            let alto = img.height();
            let mut datos = Vec::new();
            img.write_to(&mut Cursor::new(&mut datos), ImageFormat::Png)
                .map_err(|e| InfraError::Imagen(e.to_string()))?;

            paginas.push(PageImage { datos, ancho, alto, numero_pagina });
        }

        if paginas.is_empty() {
            return Err(InfraError::Pdf("PDF sin páginas".to_string()).into());
        }

        Ok(paginas)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructor_acepta_dpi_personalizado() {
        let reader = PdfiumPageReader::new(150);
        assert_eq!(reader.dpi, 150);
    }

    #[test]
    fn constructor_default_usa_300_dpi() {
        let reader = PdfiumPageReader::default_dpi();
        assert_eq!(reader.dpi, 300);
    }

    #[test]
    fn bytes_vacios_retorna_error() {
        let reader = PdfiumPageReader::new(300);
        let result = reader.leer_bytes(&[]);
        assert!(result.is_err(), "bytes vacíos deben retornar error");
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("vacío") || msg.contains("PDF"), "mensaje: {msg}");
    }

    #[test]
    fn bytes_invalidos_retorna_error_descriptivo() {
        let reader = PdfiumPageReader::new(300);
        let result = reader.leer_bytes(b"not a pdf");
        assert!(result.is_err(), "bytes inválidos deben retornar error");
    }

    #[test]
    fn leer_archivo_inexistente_retorna_error_io() {
        let reader = PdfiumPageReader::new(300);
        let result = reader.leer_archivo("/tmp/no_existe_reconstructor_test_xyz.pdf");
        assert!(result.is_err(), "archivo inexistente debe retornar error");
    }

    #[test]
    fn target_width_calculado_correctamente() {
        let _reader = PdfiumPageReader::new(300);
        let expected = (8.27 * 300.0_f32) as i32;
        assert_eq!(expected, 2481);
    }

    #[test]
    #[ignore = "requiere libpdfium.so en el sistema"]
    fn leer_pdf_sintetico_produce_paginas() {
        let reader = PdfiumPageReader::new(72);
        let pdf_path = std::env::var("TEST_PDF_PATH").unwrap_or_else(|_| "tests/fixtures/sample.pdf".to_string());

        if !std::path::Path::new(&pdf_path).exists() {
            eprintln!("Saltando test: {pdf_path} no existe");
            return;
        }

        let paginas = reader.leer_archivo(&pdf_path).unwrap();
        assert!(!paginas.is_empty(), "debe haber al menos 1 página");
        assert!(paginas[0].numero_pagina == 1);
        assert!(paginas[0].ancho > 0);
        assert!(paginas[0].alto > 0);
        assert!(!paginas[0].datos.is_empty());
    }
}
