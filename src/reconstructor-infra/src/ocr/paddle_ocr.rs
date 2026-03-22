// =============================================================================
// PaddleOcrResolver — Reconocimiento Óptico de Caracteres en 2 Fases
//
// Propósito: Interfaz del dominio hacia la topología PP-OCRv5. 
//            Encapsula un sub-pipeline propio (Detección DBNet -> Reconocimiento SVTR)
//            escondiendo esta complejidad al Orquestador general y garantizando que 
//            cada bloque retorne un String plano estructurado.
// =============================================================================

use std::io::Cursor;
use std::sync::{Arc, Mutex};

use image::{DynamicImage, ImageFormat, imageops};
use ndarray::Array4;
use ort::session::Session;
use ort::value::TensorRef;
use reconstructor_domain::traits::{BlockResolver, TextlineOrientationCorrector};
use reconstructor_domain::{BlockType, DomainError, Region, ResolvedContent};

use crate::error::InfraError;
use crate::ort_session::construir_sesion;

/// Orquesta internamente la secuencia DBNet (Segmentación de líneas) -> SVTR (Decodificación).
///
/// Soporta la inyección opcional de un corrector de orientación horizontal (`TextlineOrientationCorrector`)
/// para procesar documentos escaneados al revés.
pub struct PaddleOcrResolver {
    session_det: Mutex<Session>,
    session_rec: Mutex<Session>,
    dict: Vec<String>,
    textline_corrector: Option<Arc<dyn TextlineOrientationCorrector>>,
}

impl PaddleOcrResolver {
    pub fn new(
        det_model_path: &str,
        rec_model_path: &str,
        dict_path: &str,
    ) -> Result<Self, InfraError> {
        Self::new_with_gpu(det_model_path, rec_model_path, dict_path, false)
    }

    /// Inicializa las redes neuronales intentando acoplarse al driver gráfico subyacente.
    ///
    /// # Arguments
    ///
    /// * `det_model_path` - Ruta al modelo clasificador DBNet.
    /// * `rec_model_path` - Ruta al modelo decodificador secuencial SVTR.
    /// * `dict_path` - Mapeo de índices a caracteres UTF-8.
    /// * `use_gpu` - Permite a `ort_session` delegar la aceleración a CUDA/TensorRT.
    pub fn new_with_gpu(
        det_model_path: &str,
        rec_model_path: &str,
        dict_path: &str,
        use_gpu: bool,
    ) -> Result<Self, InfraError> {
        let session_det = construir_sesion(det_model_path, use_gpu)
            .map_err(|e| InfraError::Onnx(e.to_string()))?;

        let session_rec = construir_sesion(rec_model_path, use_gpu)
            .map_err(|e| InfraError::Onnx(e.to_string()))?;

        let dict_content =
            std::fs::read_to_string(dict_path).map_err(InfraError::Io)?;
        let dict: Vec<String> = std::iter::once(" ".to_string())
            .chain(dict_content.lines().map(|l| l.to_string()))
            .chain(std::iter::once(" ".to_string()))
            .collect();

        Ok(Self {
            session_det: Mutex::new(session_det),
            session_rec: Mutex::new(session_rec),
            dict,
            textline_corrector: None,
        })
    }

    /// Implementa el patrón Builder para inyectar corrección de orientación de nivel 2.
    pub fn with_textline_corrector(
        mut self,
        corrector: Box<dyn TextlineOrientationCorrector>,
    ) -> Self {
        self.textline_corrector = Some(Arc::from(corrector));
        self
    }
}

impl BlockResolver for PaddleOcrResolver {
    fn puede_resolver(&self, tipo: BlockType) -> bool {
        tipo.es_textual()
    }

    /// Ejecuta el OCR en dos fases sobre un bloque previamente clasificado como texto.
    ///
    /// # Arguments
    ///
    /// * `region` - Metadatos inmutables del bounding box a resolver.
    /// * `crop_bytes` - Buffer gráfico asilado para prevenir sobrecarga de memoria del host.
    ///
    /// # Errors
    ///
    /// Retorna `DomainError` si las inferencias de red causan pánico o el Mutex sufre envenenamiento
    /// tras un aborto en hilos paralelos (Rayon).
    fn resolver(
        &self,
        _region: &Region,
        crop_bytes: &[u8],
    ) -> Result<(ResolvedContent, f32), DomainError> {
        if crop_bytes.is_empty() {
            return Ok((ResolvedContent::Text { texto: String::new() }, 0.0));
        }

        let img = image::load_from_memory(crop_bytes)
            .map_err(|e| InfraError::Imagen(e.to_string()))?;

        let lineas_crops = {
            let mut session = self
                .session_det
                .lock()
                .map_err(|e| InfraError::Onnx(format!("Mutex poisoned: {e}")))?;
            detectar_lineas(&mut session, &img)?
        };

        if lineas_crops.is_empty() {
            return Ok((ResolvedContent::Text { texto: String::new() }, 0.0));
        }

        let lineas_crops = if let Some(ref corrector) = self.textline_corrector {
            let crops_bytes: Vec<Vec<u8>> = lineas_crops
                .iter()
                .map(|img| {
                    let mut buf = Vec::new();
                    img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
                        .map_err(|e| InfraError::Imagen(e.to_string()))
                        .map_err(DomainError::from)?;
                    Ok(buf)
                })
                .collect::<Result<Vec<_>, DomainError>>()?;

            let resultados = corrector.corregir_lineas(&crops_bytes)?;

            lineas_crops
                .into_iter()
                .zip(resultados)
                .map(|(img, resultado)| {
                    if resultado.angulo_grados == 180.0 {
                        DynamicImage::ImageRgba8(imageops::rotate180(&img.to_rgba8()))
                    } else {
                        img
                    }
                })
                .collect()
        } else {
            lineas_crops
        };

        let mut textos = Vec::new();
        let mut confianza_total = 0.0f32;

        for linea in &lineas_crops {
            let mut session = self
                .session_rec
                .lock()
                .map_err(|e| InfraError::Onnx(format!("Mutex poisoned: {e}")))?;
            let (texto, conf) = reconocer_linea(&mut session, linea, &self.dict)?;
            if !texto.is_empty() {
                textos.push(texto);
                confianza_total += conf;
            }
        }

        let confianza_media = if textos.is_empty() {
            0.0
        } else {
            confianza_total / textos.len() as f32
        };

        Ok((
            ResolvedContent::Text { texto: textos.join("\n") },
            confianza_media,
        ))
    }
}

fn detectar_lineas(
    session: &mut Session,
    img: &DynamicImage,
) -> Result<Vec<DynamicImage>, DomainError> {
    let (orig_w, orig_h) = (img.width(), img.height());

    let target_w = ((orig_w + 31) / 32 * 32).min(960);
    let target_h = ((orig_h + 31) / 32 * 32).min(960);

    let resized = img.resize_exact(
        target_w,
        target_h,
        image::imageops::FilterType::CatmullRom,
    );
    let rgb = resized.to_rgb8();

    let tw = target_w as usize;
    let th = target_h as usize;
    let mut tensor = Array4::<f32>::zeros([1, 3, th, tw]);

    const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
    const STD: [f32; 3] = [0.229, 0.224, 0.225];

    for (y, row) in rgb.rows().enumerate() {
        for (x, pixel) in row.enumerate() {
            let [r, g, b] = pixel.0;
            tensor[[0, 0, y, x]] = (r as f32 / 255.0 - MEAN[0]) / STD[0];
            tensor[[0, 1, y, x]] = (g as f32 / 255.0 - MEAN[1]) / STD[1];
            tensor[[0, 2, y, x]] = (b as f32 / 255.0 - MEAN[2]) / STD[2];
        }
    }

    let tensor_ref = TensorRef::<f32>::from_array_view(tensor.view())
        .map_err(|e| InfraError::Onnx(e.to_string()))?;

    let outputs = session
        .run(ort::inputs!["x" => tensor_ref])
        .map_err(|e| InfraError::Onnx(e.to_string()))?;

    let out_array = outputs[0]
        .try_extract_array::<f32>()
        .map_err(|e| InfraError::Onnx(e.to_string()))?;

    let data = out_array
        .as_slice()
        .ok_or_else(|| InfraError::Onnx("Output no contiguo".to_string()))?;

    const DET_THRESHOLD: f32 = 0.3;
    let h = th;
    let w = tw;

    let mut lineas_crops = Vec::new();
    let mut y_inicio: Option<usize> = None;

    for y in 0..h {
        let fila_activa = (0..w).any(|x| {
            let idx = y * w + x;
            idx < data.len() && data[idx] > DET_THRESHOLD
        });
        match (fila_activa, y_inicio) {
            (true, None) => y_inicio = Some(y),
            (false, Some(y0)) => {
                let x0 = (0..w)
                    .find(|&x| {
                        (y0..y).any(|r| {
                            let idx = r * w + x;
                            idx < data.len() && data[idx] > DET_THRESHOLD
                        })
                    })
                    .unwrap_or(0);
                let x1 = (0..w)
                    .rev()
                    .find(|&x| {
                        (y0..y).any(|r| {
                            let idx = r * w + x;
                            idx < data.len() && data[idx] > DET_THRESHOLD
                        })
                    })
                    .unwrap_or(w.saturating_sub(1));

                let scale_x = orig_w as f32 / target_w as f32;
                let scale_y = orig_h as f32 / target_h as f32;
                let ox0 = ((x0 as f32) * scale_x) as u32;
                let oy0 = ((y0 as f32) * scale_y) as u32;
                let ow = (((x1 - x0 + 1) as f32) * scale_x) as u32;
                let oh = (((y - y0) as f32) * scale_y) as u32;

                if ow > 5 && oh > 5 {
                    let crop = img.crop_imm(
                        ox0.min(orig_w.saturating_sub(1)),
                        oy0.min(orig_h.saturating_sub(1)),
                        ow.min(orig_w.saturating_sub(ox0)),
                        oh.min(orig_h.saturating_sub(oy0)),
                    );
                    lineas_crops.push(crop);
                }
                y_inicio = None;
            }
            _ => {}
        }
    }

    Ok(lineas_crops)
}

fn reconocer_linea(
    session: &mut Session,
    linea: &DynamicImage,
    dict: &[String],
) -> Result<(String, f32), DomainError> {
    const REC_HEIGHT: u32 = 48;

    let (orig_w, orig_h) = (linea.width(), linea.height());
    let target_w = (orig_w * REC_HEIGHT / orig_h.max(1)).max(1);
    let resized = linea.resize_exact(
        target_w,
        REC_HEIGHT,
        image::imageops::FilterType::CatmullRom,
    );
    let rgb = resized.to_rgb8();

    let tw = target_w as usize;
    let th = REC_HEIGHT as usize;
    let mut tensor = Array4::<f32>::zeros([1, 3, th, tw]);

    for (y, row) in rgb.rows().enumerate() {
        for (x, pixel) in row.enumerate() {
            let [r, g, b] = pixel.0;
            tensor[[0, 0, y, x]] = (r as f32 / 255.0 - 0.5) / 0.5;
            tensor[[0, 1, y, x]] = (g as f32 / 255.0 - 0.5) / 0.5;
            tensor[[0, 2, y, x]] = (b as f32 / 255.0 - 0.5) / 0.5;
        }
    }

    let tensor_ref = TensorRef::<f32>::from_array_view(tensor.view())
        .map_err(|e| InfraError::Onnx(e.to_string()))?;

    let outputs = session
        .run(ort::inputs!["x" => tensor_ref])
        .map_err(|e| InfraError::Onnx(e.to_string()))?;

    let out_array = outputs[0]
        .try_extract_array::<f32>()
        .map_err(|e| InfraError::Onnx(e.to_string()))?;

    let shape = out_array.shape().to_vec();
    if shape.len() < 3 {
        return Ok((String::new(), 0.0));
    }
    let (t, vocab) = (shape[1], shape[2]);

    let data = out_array.as_slice().ok_or_else(|| {
        InfraError::Onnx("Rec output no contiguo".to_string())
    })?;

    let mut texto = String::new();
    let mut ultimo_idx = vocab;
    let mut scores: Vec<f32> = Vec::new();

    for step in 0..t {
        let base = step * vocab;
        let logits = &data[base..base + vocab];

        let max_val = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let exp_sum: f32 = logits.iter().map(|&x| (x - max_val).exp()).sum();
        let probs: Vec<f32> =
            logits.iter().map(|&x| (x - max_val).exp() / exp_sum).collect();

        let (idx, &prob) = probs
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap();

        if idx != 0 && idx != ultimo_idx {
            if let Some(ch) = dict.get(idx) {
                texto.push_str(ch.as_str());
                scores.push(prob);
            }
        }
        ultimo_idx = idx;
    }

    let confianza = if scores.is_empty() {
        0.0
    } else {
        scores.iter().sum::<f32>() / scores.len() as f32
    };

    Ok((texto, confianza))
}
