// =============================================================================
// PrintPdfComposer — Ensamblador Vectorial de Documentos
//
// Propósito: Desacoplar el dominio del formato nativo PDF y la librería externa `printpdf`.
//            Garantiza que la reconstrucción ("No PDF Sandwich") proyecte entidades 
//            semánticas en capas vectoriales seleccionables, traduciendo de la escala raster 
//            nativa a la escala tipográfica PDF (puntos).
// =============================================================================

use std::io::{BufWriter, Cursor};

use printpdf::{BuiltinFont, Image, ImageTransform, Mm, PdfDocument};
use reconstructor_domain::traits::{ComposedPage, PageComposer};
use reconstructor_domain::{DomainError, Page, ResolvedBlock, ResolvedContent};

use crate::error::InfraError;

const DPI: f32 = 300.0;

#[inline]
fn px_a_mm(px: f32) -> Mm {
    Mm(px * 25.4 / DPI)
}

/// Implementación del rendering backend que dibuja el documento "from scratch".
pub struct PrintPdfComposer;

impl PageComposer for PrintPdfComposer {
    /// Transmuta el grafo de bloques semánticos a un blob PDF independiente.
    ///
    /// # Arguments
    ///
    /// * `pagina` - Metadatos inmutables requeridos para extrapolar los sistemas de coordenadas 
    ///   de origen (Top-Left, Píxeles) al sistema destino PDF (Bottom-Left, Milímetros).
    /// * `bloques` - Grafo de entidades lógicas resueltas por las FSM.
    ///
    /// # Errors
    ///
    /// Propaga un `DomainError` (InfraError) si las fuentes embebidas no pueden inicializarse 
    /// o si el buffer de salida agota la memoria del host.
    fn componer(
        &self,
        pagina: &Page,
        bloques: &[ResolvedBlock],
    ) -> Result<ComposedPage, DomainError> {
        let ancho_mm = px_a_mm(pagina.ancho as f32);
        let alto_mm = px_a_mm(pagina.alto as f32);

        let (doc, page_idx, layer_idx) =
            PdfDocument::new("Reconstructed", ancho_mm, alto_mm, "Capa 1");
        let layer = doc.get_page(page_idx).get_layer(layer_idx);

        let font = doc
            .add_builtin_font(BuiltinFont::Helvetica)
            .map_err(|e| InfraError::Pdf(e.to_string()))?;

        let mut texto_pagina = String::new();

        for bloque in bloques {
            let region_bbox = pagina
                .regiones
                .iter()
                .find(|r| r.id == bloque.region_id)
                .map(|r| &r.bbox);

            if let Some(bbox) = region_bbox {
                let x_mm = px_a_mm(bbox.x);
                let y_pdf_mm =
                    px_a_mm(pagina.alto as f32 - bbox.y - bbox.height);

                match &bloque.contenido {
                    ResolvedContent::Text { texto } => {
                        layer.use_text(texto, 10.0, x_mm, y_pdf_mm, &font);
                        if !texto_pagina.is_empty() {
                            texto_pagina.push(' ');
                        }
                        texto_pagina.push_str(texto);
                    }

                    ResolvedContent::Table { datos } => {
                        let mut y_celda = y_pdf_mm;
                        for fila in &datos.cells {
                            let fila_txt = fila.join(" | ");
                            layer.use_text(&fila_txt, 8.0, x_mm, y_celda, &font);
                            y_celda = Mm(y_celda.0 - 5.0);
                            if !texto_pagina.is_empty() {
                                texto_pagina.push('\n');
                            }
                            texto_pagina.push_str(&fila_txt);
                        }
                    }

                    ResolvedContent::Raster { imagen_bytes, .. } => {
                        if !imagen_bytes.is_empty() {
                            if let Ok(img) =
                                printpdf::image_crate::load_from_memory(imagen_bytes)
                            {
                                let ancho_img = img.width();
                                let alto_img = img.height();
                                let pdf_image = Image::from_dynamic_image(&img);
                                pdf_image.add_to_layer(
                                    layer.clone(),
                                    ImageTransform {
                                        translate_x: Some(x_mm),
                                        translate_y: Some(y_pdf_mm),
                                        scale_x: Some(
                                            bbox.width / ancho_img as f32,
                                        ),
                                        scale_y: Some(
                                            bbox.height / alto_img as f32,
                                        ),
                                        dpi: Some(DPI),
                                        rotate: None,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }

        let mut pdf_bytes = Vec::new();
        doc.save(&mut BufWriter::new(Cursor::new(&mut pdf_bytes)))
            .map_err(|e| InfraError::Pdf(e.to_string()))?;

        Ok(ComposedPage {
            numero_pagina: pagina.numero_pagina,
            pdf_bytes,
            texto_extraido: texto_pagina,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reconstructor_domain::{
        BoundingBox, BlockType, Page, Region, ResolvedBlock, ResolvedContent,
        StrategyKind,
    };
    use reconstructor_domain::fsm::block::BlockState;

    fn pagina_test() -> Page {
        Page {
            numero_pagina: 1,
            ancho: 2480,
            alto: 3508,
            orientacion_correccion_grados: 0.0,
            orientacion_incierta: false,
            regiones: vec![Region::new(
                "blk_1_0",
                BlockType::Text,
                BoundingBox::new(100.0, 100.0, 400.0, 50.0),
                0.95,
            )],
            bloques_resueltos: vec![],
            tiempo_procesamiento_ms: 0.0,
        }
    }

    fn bloque_texto(texto: &str) -> ResolvedBlock {
        ResolvedBlock {
            region_id: "blk_1_0".to_string(),
            tipo_bloque: BlockType::Text,
            contenido: ResolvedContent::Text { texto: texto.to_string() },
            confianza_resolucion: 0.95,
            estrategia_utilizada: StrategyKind::PaddleOcr,
            estado_actual: BlockState::Composed,
            historial_estados: vec![],
        }
    }

    #[test]
    fn componer_pagina_con_texto_genera_pdf_no_vacio() {
        let composer = PrintPdfComposer;
        let pagina = pagina_test();
        let bloques = vec![bloque_texto("Hola mundo")];

        let result = composer.componer(&pagina, &bloques).unwrap();

        assert!(!result.pdf_bytes.is_empty(), "PDF no debe estar vacío");
        assert_eq!(result.numero_pagina, 1);
        assert!(result.texto_extraido.contains("Hola mundo"));
    }

    #[test]
    fn componer_pagina_vacia_genera_pdf_valido() {
        let composer = PrintPdfComposer;
        let pagina = pagina_test();

        let result = composer.componer(&pagina, &[]).unwrap();

        assert!(!result.pdf_bytes.is_empty());
        assert_eq!(result.numero_pagina, 1);
    }
}
