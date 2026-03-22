// =============================================================================
// ImageInputReader — Adaptador de Ingesta Rasterizada Directa
//
// Propósito: Interfaz estandarizada para ingestar formatos visuales puros (PNG/JPEG)
//            sin la penalización de carga de un rasterizador PDF en memoria, cumpliendo 
//            con el contrato de aislamiento del Dominio `Vec<PageImage>`.
// =============================================================================

use std::io::Cursor;
use std::path::Path;

use image::{DynamicImage, ImageFormat};
use reconstructor_domain::{DomainError, PageImage};

use crate::error::InfraError;

/// Motor de carga para imágenes estáticas.
pub struct ImageInputReader;

impl ImageInputReader {
    /// Transmuta un archivo físico en memoria rasterizada delegando la detección 
    /// de formato al `image-rs`.
    ///
    /// Excluye expresamente los binarios PDF, enrutando ese flujo hacia `PdfiumPageReader`.
    ///
    /// # Arguments
    ///
    /// * `ruta` - Ubicación en el sistema de archivos del SO.
    ///
    /// # Errors
    ///
    /// Retorna `DomainError` si el archivo especificado es un PDF, o si la I/O nativa falla.
    pub fn leer_archivo(&self, ruta: &str) -> Result<Vec<PageImage>, DomainError> {
        let extension = Path::new(ruta)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        if extension == "pdf" {
            return Err(InfraError::Pdf(
                "pdfium no disponible en esta compilación".to_string(),
            )
            .into());
        }

        let bytes = std::fs::read(ruta).map_err(InfraError::Io)?;
        self.leer_bytes(&bytes, &extension)
    }

    /// Descodifica un blob binario en la estructura de datos unificada del Dominio.
    pub fn leer_bytes(
        &self,
        bytes: &[u8],
        extension: &str,
    ) -> Result<Vec<PageImage>, DomainError> {
        let fmt = match extension {
            "png" => Some(ImageFormat::Png),
            "jpg" | "jpeg" => Some(ImageFormat::Jpeg),
            "webp" => Some(ImageFormat::WebP),
            "tiff" | "tif" => Some(ImageFormat::Tiff),
            _ => None,
        };

        let img = if let Some(f) = fmt {
            image::load_from_memory_with_format(bytes, f)
                .map_err(|e| InfraError::Imagen(e.to_string()))?
        } else {
            image::load_from_memory(bytes)
                .map_err(|e| InfraError::Imagen(e.to_string()))?
        };

        let page_image = dynamic_a_page_image(img, 1)?;
        Ok(vec![page_image])
    }
}

fn dynamic_a_page_image(
    img: DynamicImage,
    numero_pagina: u32,
) -> Result<PageImage, DomainError> {
    let ancho = img.width();
    let alto = img.height();

    let mut datos = Vec::new();
    img.write_to(&mut Cursor::new(&mut datos), ImageFormat::Png)
        .map_err(|e| InfraError::Imagen(e.to_string()))?;

    Ok(PageImage { datos, ancho, alto, numero_pagina })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crear_png(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbImage::new(width, height);
        let dyn_img = DynamicImage::ImageRgb8(img);
        let mut bytes = Vec::new();
        dyn_img
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn leer_png_sintetico() {
        let reader = ImageInputReader;
        let png = crear_png(100, 200);
        let paginas = reader.leer_bytes(&png, "png").unwrap();

        assert_eq!(paginas.len(), 1);
        assert_eq!(paginas[0].ancho, 100);
        assert_eq!(paginas[0].alto, 200);
        assert_eq!(paginas[0].numero_pagina, 1);
        assert!(!paginas[0].datos.is_empty());
    }

    #[test]
    fn leer_pdf_retorna_error() {
        let reader = ImageInputReader;
        let result = reader.leer_archivo("documento.pdf");
        assert!(result.is_err());
    }

    #[test]
    fn leer_jpeg_sintetico() {
        let reader = ImageInputReader;
        let img = DynamicImage::ImageRgb8(image::RgbImage::new(50, 50));
        let mut bytes = Vec::new();
        img.write_to(&mut Cursor::new(&mut bytes), ImageFormat::Jpeg).unwrap();

        let paginas = reader.leer_bytes(&bytes, "jpg").unwrap();
        assert_eq!(paginas.len(), 1);
        assert_eq!(paginas[0].numero_pagina, 1);
    }

    #[test]
    fn extension_desconocida_usa_autodetect() {
        let reader = ImageInputReader;
        let png = crear_png(30, 30);
        let paginas = reader.leer_bytes(&png, "xyz").unwrap();
        assert_eq!(paginas.len(), 1);
    }
}
