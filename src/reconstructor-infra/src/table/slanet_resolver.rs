// =============================================================================
// SlaNetTableResolver — Inferencia Estructural Bidimensional (F1.7)
//
// Propósito: Desacoplar la lógica de parsing de grillas lógicas (HTML) y bounding 
//            boxes del algoritmo de renderizado de la UI/PDF.
//            Abstrae el modelo seq2seq `SLANet+` para que el Dominio trate las 
//            tablas como estructuras de datos puras (filas y columnas).
// =============================================================================

use std::sync::Mutex;

use ndarray::Array4;
use ort::session::Session;
use ort::value::TensorRef;
use reconstructor_domain::traits::BlockResolver;
use reconstructor_domain::{BoundingBox, BlockType, DomainError, Region, ResolvedContent, TableData};

use crate::error::InfraError;
use crate::ort_session::construir_sesion;

const INPUT_SIZE: u32 = 488;

const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

#[allow(dead_code)]
const TOKENS: &[&str] = &[
    "<pad>", "<unk>", "<eos>",
    "<tr>", "</tr>",
    "<td>", "</td>",
    "<td colspan=2>", "<td colspan=3>", "<td colspan=4>", "<td colspan=5>",
    "<td rowspan=2>", "<td rowspan=3>", "<td rowspan=4>", "<td rowspan=5>",
    "<td colspan=2 rowspan=2>", "<td colspan=3 rowspan=2>", "<td colspan=2 rowspan=3>",
    "<thead>", "</thead>", "<tbody>", "</tbody>",
];

const TOKEN_PAD: usize = 0;
const TOKEN_EOS: usize = 2;
const TOKEN_TR: usize = 3;
const TOKEN_TR_CLOSE: usize = 4;

/// Interfaz del dominio hacia el modelo SLANet+ ONNX Runtime.
///
/// Mantiene la sesión ML bajo un candado (Mutex) para garantizar el uso thread-safe
/// a través de la fábrica de resolución inyectada en el `PipelineOrchestrator`.
pub struct SlaNetTableResolver {
    session: Mutex<Session>,
}

impl SlaNetTableResolver {
    pub fn new(model_path: &str) -> Result<Self, InfraError> {
        Self::new_with_gpu(model_path, false)
    }

    /// Inicializa la red neuronal intentando acoplarse al driver gráfico subyacente.
    ///
    /// # Arguments
    ///
    /// * `model_path` - Ruta al archivo `.onnx` verificado.
    /// * `use_gpu` - Permite a `ort_session` delegar la aceleración a CUDA/TensorRT.
    pub fn new_with_gpu(model_path: &str, use_gpu: bool) -> Result<Self, InfraError> {
        let session = construir_sesion(model_path, use_gpu)
            .map_err(|e| InfraError::Onnx(e.to_string()))?;
        Ok(Self { session: Mutex::new(session) })
    }

    fn preprocesar(crop_bytes: &[u8]) -> Result<Array4<f32>, InfraError> {
        let img = image::load_from_memory(crop_bytes)
            .map_err(|e| InfraError::Imagen(e.to_string()))?;

        let s = INPUT_SIZE as usize;
        let resized = img.resize_exact(INPUT_SIZE, INPUT_SIZE, image::imageops::FilterType::CatmullRom);
        let rgb = resized.to_rgb8();

        let mut tensor = Array4::<f32>::zeros([1, 3, s, s]);
        for (y, row) in rgb.rows().enumerate() {
            for (x, pixel) in row.enumerate() {
                let [r, g, b] = pixel.0;
                tensor[[0, 0, y, x]] = (r as f32 / 255.0 - MEAN[0]) / STD[0];
                tensor[[0, 1, y, x]] = (g as f32 / 255.0 - MEAN[1]) / STD[1];
                tensor[[0, 2, y, x]] = (b as f32 / 255.0 - MEAN[2]) / STD[2];
            }
        }
        Ok(tensor)
    }

    fn decodificar_greedy(structure_probs: &[f32], max_len: usize, num_classes: usize) -> (Vec<usize>, Vec<f32>) {
        let mut tokens = Vec::new();
        let mut probs = Vec::new();

        for i in 0..max_len {
            let offset = i * num_classes;
            let slice = &structure_probs[offset..offset + num_classes];
            let (best_idx, &best_prob) = slice
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
                .unwrap_or((0, &0.0));
            tokens.push(best_idx);
            probs.push(best_prob);
            if best_idx == TOKEN_EOS {
                break;
            }
        }
        (tokens, probs)
    }

    fn es_token_td(token_idx: usize) -> bool {
        matches!(token_idx, 5 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15 | 16 | 17)
    }

    fn colspan_de_token(token_idx: usize) -> u32 {
        match token_idx {
            7 => 2, 
            8 => 3, 
            9 => 4, 
            10 => 5, 
            15 => 2, 
            16 => 3, 
            17 => 2, 
            _ => 1,
        }
    }

    fn parsear_estructura(
        tokens: &[usize],
        probs: &[f32],
        loc: &[f32],
        ancho_original: u32,
        alto_original: u32,
    ) -> Result<(TableData, f32), InfraError> {
        if tokens.is_empty() {
            return Err(InfraError::Onnx("Secuencia de tokens vacía".to_string()));
        }

        let mut filas: u32 = 0;
        let mut max_cols: u32 = 0;
        let mut cols_en_fila_actual: u32 = 0;
        let mut in_row = false;
        let mut celdas_bbox: Vec<BoundingBox> = Vec::new();

        for (i, &tok) in tokens.iter().enumerate() {
            if tok == TOKEN_TR {
                if in_row && cols_en_fila_actual > max_cols {
                    max_cols = cols_en_fila_actual;
                }
                in_row = true;
                cols_en_fila_actual = 0;
                filas += 1;
            } else if tok == TOKEN_TR_CLOSE {
                if cols_en_fila_actual > max_cols {
                    max_cols = cols_en_fila_actual;
                }
                in_row = false;
            } else if Self::es_token_td(tok) {
                let cs = Self::colspan_de_token(tok);
                cols_en_fila_actual += cs;

                let loc_offset = i * 4;
                if loc_offset + 3 < loc.len() {
                    let x1 = loc[loc_offset].clamp(0.0, 1.0);
                    let y1 = loc[loc_offset + 1].clamp(0.0, 1.0);
                    let x2 = loc[loc_offset + 2].clamp(0.0, 1.0);
                    let y2 = loc[loc_offset + 3].clamp(0.0, 1.0);
                    let bbox = BoundingBox::new(
                        x1 * ancho_original as f32,
                        y1 * alto_original as f32,
                        (x2 - x1) * ancho_original as f32,
                        (y2 - y1) * alto_original as f32,
                    );
                    celdas_bbox.push(bbox);
                }
            }
        }
        if in_row && cols_en_fila_actual > max_cols {
            max_cols = cols_en_fila_actual;
        }

        if filas == 0 || max_cols == 0 {
            return Err(InfraError::Onnx(format!(
                "Estructura de tabla vacía: filas={filas}, columnas={max_cols}"
            )));
        }

        let cells: Vec<Vec<String>> = (0..filas as usize)
            .map(|_| vec![String::new(); max_cols as usize])
            .collect();

        let non_pad: Vec<f32> = probs
            .iter()
            .zip(tokens.iter())
            .filter(|(_, &t)| t != TOKEN_PAD)
            .map(|(&p, _)| p)
            .collect();

        let confianza = if non_pad.is_empty() {
            0.0
        } else {
            let avg = non_pad.iter().sum::<f32>() / non_pad.len() as f32;
            avg.clamp(0.0, 0.95)
        };

        Ok((TableData { cells, filas, columnas: max_cols, celdas_bbox }, confianza))
    }
}

impl BlockResolver for SlaNetTableResolver {
    fn puede_resolver(&self, tipo: BlockType) -> bool {
        tipo.es_tabla()
    }

    /// Implementa el contrato `BlockResolver` para inferir una matriz tabular desde píxeles.
    ///
    /// # Arguments
    ///
    /// * `region` - Metadatos geométricos que acotan la inferencia.
    /// * `crop_bytes` - Buffer de imagen precortado por el Orquestador para evitar OOM.
    ///
    /// # Errors
    ///
    /// Devuelve `DomainError` si el payload agota la RAM o si `ort` falla 
    /// produciendo formas de tensor dispares.
    fn resolver(
        &self,
        region: &Region,
        crop_bytes: &[u8],
    ) -> Result<(ResolvedContent, f32), DomainError> {
        if crop_bytes.is_empty() {
            return Err(DomainError::Infraestructura(
                "SlaNetTableResolver: crop vacío".to_string(),
            ));
        }

        let img_original = image::load_from_memory(crop_bytes)
            .map_err(|e| InfraError::Imagen(e.to_string()))?;
        let ancho_original = img_original.width();
        let alto_original = img_original.height();
        drop(img_original);

        let tensor = Self::preprocesar(crop_bytes)
            .map_err(DomainError::from)?;

        let tensor_ref = TensorRef::<f32>::from_array_view(tensor.view())
            .map_err(|e| InfraError::Onnx(e.to_string()))
            .map_err(DomainError::from)?;

        let mut session = self
            .session
            .lock()
            .map_err(|e| DomainError::Infraestructura(format!("Mutex poisoned: {e}")))?;

        let outputs = session
            .run(ort::inputs!["x" => tensor_ref])
            .map_err(|e| InfraError::Onnx(e.to_string()))
            .map_err(DomainError::from)?;

        let structure_tensor = outputs[0]
            .try_extract_array::<f32>()
            .map_err(|e| InfraError::Onnx(e.to_string()))
            .map_err(DomainError::from)?;

        let structure_shape = structure_tensor.shape().to_vec();
        if structure_shape.len() < 3 {
            return Err(DomainError::Infraestructura(
                "structure_probs shape inesperado".to_string(),
            ));
        }
        let max_len = structure_shape[1];
        let num_classes = structure_shape[2];

        let structure_slice = structure_tensor.as_slice().ok_or_else(|| {
            DomainError::Infraestructura("structure_probs no contiguo".to_string())
        })?;

        let loc_tensor = outputs[1]
            .try_extract_array::<f32>()
            .map_err(|e| InfraError::Onnx(e.to_string()))
            .map_err(DomainError::from)?;

        let loc_slice = loc_tensor.as_slice().ok_or_else(|| {
            DomainError::Infraestructura("loc no contiguo".to_string())
        })?;

        let (token_indices, probs) =
            Self::decodificar_greedy(structure_slice, max_len, num_classes);

        tracing::debug!(
            "SlaNetTableResolver [{}]: {} tokens decodificados",
            region.id,
            token_indices.len()
        );

        let (table_data, confianza) =
            Self::parsear_estructura(&token_indices, &probs, loc_slice, ancho_original, alto_original)
                .map_err(DomainError::from)?;

        tracing::debug!(
            "SlaNetTableResolver [{}]: {}x{} tabla, confianza={:.3}",
            region.id,
            table_data.filas,
            table_data.columnas,
            confianza
        );

        Ok((ResolvedContent::Table { datos: table_data }, confianza))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reconstructor_domain::{BoundingBox, BlockType, Region};

    fn region_tabla() -> Region {
        Region::new(
            "blk_1_5",
            BlockType::Table,
            BoundingBox::new(50.0, 200.0, 500.0, 300.0),
            0.88,
        )
    }

    fn png_sintetico() -> Vec<u8> {
        use image::{ImageBuffer, Rgb};
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
            ImageBuffer::from_fn(10, 10, |_, _| Rgb([255u8, 255, 255]));
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        buf
    }

    #[test]
    fn puede_resolver_solo_tablas() {
        struct Stub;
        impl Stub {
            fn puede_resolver(&self, tipo: BlockType) -> bool {
                tipo.es_tabla()
            }
        }
        let s = Stub;
        assert!(s.puede_resolver(BlockType::Table));
        assert!(!s.puede_resolver(BlockType::Text));
        assert!(!s.puede_resolver(BlockType::Figure));
    }

    #[test]
    fn preprocesar_imagen_produce_tensor_correcto() {
        let png = png_sintetico();
        let tensor = SlaNetTableResolver::preprocesar(&png).unwrap();

        let shape = tensor.shape();
        assert_eq!(shape, &[1, 3, 488, 488]);

        let val_r = tensor[[0, 0, 0, 0]];
        let expected = (1.0_f32 - MEAN[0]) / STD[0];
        assert!((val_r - expected).abs() < 1e-4, "val_r={val_r}, expected={expected}");
    }

    #[test]
    fn parsear_estructura_tabla_simple() {
        let tokens = vec![3, 5, 6, 5, 6, 4]; 
        let probs = vec![0.9_f32; tokens.len()];
        let loc = vec![0.0_f32, 0.0, 0.5, 0.5, 
                       0.0, 0.0, 0.5, 1.0,  
                       0.0, 0.0, 0.0, 0.0, 
                       0.5, 0.0, 1.0, 1.0, 
                       0.0, 0.0, 0.0, 0.0,
                       0.0, 0.0, 0.0, 0.0];

        let (table_data, confianza) =
            SlaNetTableResolver::parsear_estructura(&tokens, &probs, &loc, 100, 50).unwrap();

        assert_eq!(table_data.filas, 1);
        assert_eq!(table_data.columnas, 2);
        assert_eq!(table_data.celdas.len(), 1);
        assert_eq!(table_data.celdas[0].len(), 2);
        assert!(confianza > 0.0 && confianza <= 0.95);
    }

    #[test]
    fn parsear_estructura_tabla_2x3() {
        let fila = vec![3usize, 5, 6, 5, 6, 5, 6, 4];
        let tokens: Vec<usize> = fila.iter().chain(fila.iter()).copied().collect();
        let probs = vec![0.85_f32; tokens.len()];
        let loc = vec![0.0_f32; tokens.len() * 4];

        let (table_data, _) =
            SlaNetTableResolver::parsear_estructura(&tokens, &probs, &loc, 200, 100).unwrap();

        assert_eq!(table_data.filas, 2);
        assert_eq!(table_data.columnas, 3);
    }

    #[test]
    fn parsear_sin_tokens_retorna_error() {
        let result = SlaNetTableResolver::parsear_estructura(&[], &[], &[], 100, 100);
        assert!(result.is_err(), "se esperaba error con secuencia vacía");
    }

    #[test]
    fn confianza_se_calcula_como_promedio() {
        let tokens = vec![3usize, 5, 6, 4];
        let probs = vec![0.8_f32, 0.9, 0.85, 0.75];
        let loc = vec![0.0_f32; tokens.len() * 4];

        let (_, confianza) =
            SlaNetTableResolver::parsear_estructura(&tokens, &probs, &loc, 100, 100).unwrap();

        let expected_avg = (0.8_f32 + 0.9 + 0.85 + 0.75) / 4.0;
        let expected = expected_avg.clamp(0.0, 0.95);
        assert!((confianza - expected).abs() < 1e-5);
    }

    #[test]
    #[ignore = "requiere models/table/slanet_plus.onnx"]
    fn resolver_tabla_sintetica_con_modelo() {
        let resolver = SlaNetTableResolver::new("models/table/slanet_plus.onnx").unwrap();
        let region = region_tabla();
        let png = png_sintetico();
        let result = resolver.resolver(&region, &png);
        match result {
            Ok((ResolvedContent::Table { datos }, conf)) => {
                assert!(datos.filas > 0);
                assert!(datos.columnas > 0);
                assert!(conf >= 0.0 && conf <= 0.95);
            }
            Err(_) => {
            }
            _ => panic!("se esperaba Table o Error"),
        }
    }
}
