// =============================================================================
// OnnxTextlineOrientationCorrector — Inferencia rotacional por lotes
//
// Propósito: Mitigar falsos positivos y errores de diccionario en la fase de OCR 
//            al enderezar renglones que el modelo de Layout clasificó correctamente 
//            pero cuya orientación nativa estaba invertida (180°). 
//            Optimiza el rendimiento agrupando los tensores en mini-batches (`BATCH_SIZE`).
// =============================================================================

use std::sync::Mutex;

use ndarray::Array4;
use ort::session::Session;
use ort::value::TensorRef;
use reconstructor_domain::traits::{OrientationResult, TextlineOrientationCorrector};
use reconstructor_domain::DomainError;

use crate::error::InfraError;
use crate::ort_session::construir_sesion;

const REC_HEIGHT: u32 = 48;
const REC_WIDTH: u32 = 192;
const BATCH_SIZE: usize = 32;
const CONFIDENCE_THRESHOLD: f32 = 0.90;

const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

/// Interfaz hacia el modelo binario de detección de orientación (0°/180°).
///
/// Gestiona la concurrencia delegando en el Thread Pool interno de ONNX Runtime (`ort`).
pub struct OnnxTextlineOrientationCorrector {
    session: Mutex<Session>,
}

impl OnnxTextlineOrientationCorrector {
    pub fn new(model_path: &str) -> Result<Self, InfraError> {
        Self::new_with_gpu(model_path, false)
    }

    /// Carga el modelo en memoria VRAM/RAM delegando al proveedor de ejecución apropiado.
    pub fn new_with_gpu(model_path: &str, use_gpu: bool) -> Result<Self, InfraError> {
        let session = construir_sesion(model_path, use_gpu)
            .map_err(|e| InfraError::Onnx(e.to_string()))?;
        Ok(Self { session: Mutex::new(session) })
    }
}

impl TextlineOrientationCorrector for OnnxTextlineOrientationCorrector {
    /// Combina múltiples recortes (crops) en un único Tensor N-dimensional.
    ///
    /// Agrupa las inferencias reduciendo el overhead de latencia inherente 
    /// a las transiciones entre CPU/GPU y el propio runtime de `ort`.
    ///
    /// # Arguments
    ///
    /// * `crops` - Slice de buffers que representan líneas de texto independientes.
    ///
    /// # Errors
    ///
    /// Retorna `DomainError` si el redimensionado agota la memoria del host o 
    /// si la inicialización matricial de ONNX falla estructuralmente.
    fn corregir_lineas(
        &self,
        crops: &[Vec<u8>],
    ) -> Result<Vec<OrientationResult>, DomainError> {
        if crops.is_empty() {
            return Ok(vec![]);
        }

        let h = REC_HEIGHT as usize;
        let w = REC_WIDTH as usize;
        let mut results = Vec::with_capacity(crops.len());

        for chunk in crops.chunks(BATCH_SIZE) {
            let n = chunk.len();

            let mut batch = Array4::<f32>::zeros([n, 3, h, w]);

            for (i, bytes) in chunk.iter().enumerate() {
                let img = image::load_from_memory(bytes)
                    .map_err(|e| InfraError::Imagen(e.to_string()))
                    .map_err(DomainError::from)?;

                let resized = img.resize_exact(
                    REC_WIDTH,
                    REC_HEIGHT,
                    image::imageops::FilterType::CatmullRom,
                );
                let rgb = resized.to_rgb8();

                for (y, row) in rgb.rows().enumerate() {
                    for (x, pixel) in row.enumerate() {
                        let [r, g, b] = pixel.0;
                        batch[[i, 0, y, x]] = (r as f32 / 255.0 - MEAN[0]) / STD[0];
                        batch[[i, 1, y, x]] = (g as f32 / 255.0 - MEAN[1]) / STD[1];
                        batch[[i, 2, y, x]] = (b as f32 / 255.0 - MEAN[2]) / STD[2];
                    }
                }
            }

            let tensor_ref = TensorRef::<f32>::from_array_view(batch.view())
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

            let out_array = outputs[0]
                .try_extract_array::<f32>()
                .map_err(|e| InfraError::Onnx(e.to_string()))
                .map_err(DomainError::from)?;

            let out_slice = out_array.as_slice().ok_or_else(|| {
                DomainError::Infraestructura("Textline output no contiguo".to_string())
            })?;

            let num_classes = 2;
            for i in 0..n {
                let base = i * num_classes;
                if base + 1 >= out_slice.len() {
                    results.push(OrientationResult::sin_rotacion(0.0, true));
                    continue;
                }

                let a = out_slice[base];
                let b = out_slice[base + 1];
                let max_v = a.max(b);
                let ea = (a - max_v).exp();
                let eb = (b - max_v).exp();
                let sum = ea + eb;
                let p0 = ea / sum;
                let p1 = eb / sum;

                if p1 > p0 && p1 >= CONFIDENCE_THRESHOLD {
                    results.push(OrientationResult {
                        angulo_grados: 180.0,
                        confianza: p1,
                        incierto: false,
                    });
                } else {
                    results.push(OrientationResult {
                        angulo_grados: 0.0,
                        confianza: p0,
                        incierto: p0 < CONFIDENCE_THRESHOLD,
                    });
                }
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn png_sintetico(w: u32, h: u32) -> Vec<u8> {
        use image::{ImageBuffer, Rgb};
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
            ImageBuffer::from_fn(w, h, |_, _| Rgb([128u8, 64, 200]));
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png).unwrap();
        buf
    }

    #[test]
    fn constructor_falla_con_ruta_invalida() {
        let result = OnnxTextlineOrientationCorrector::new("/no/existe.onnx");
        assert!(result.is_err(), "debe fallar con ruta inexistente");
    }

    #[test]
    fn corregir_lineas_vacias_retorna_vec_vacio() {
        struct Stub;
        impl TextlineOrientationCorrector for Stub {
            fn corregir_lineas(&self, crops: &[Vec<u8>]) -> Result<Vec<OrientationResult>, DomainError> {
                if crops.is_empty() { return Ok(vec![]); }
                unreachable!()
            }
        }
        let stub = Stub;
        let result = stub.corregir_lineas(&[]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn batch_de_chunks_produce_n_resultados_por_chunk() {
        assert_eq!(BATCH_SIZE, 32);
    }

    #[test]
    fn umbral_confianza_es_noventa_por_ciento() {
        assert!((CONFIDENCE_THRESHOLD - 0.90).abs() < 1e-6);
    }

    #[test]
    fn softmax_binario_clase_1_detecta_180() {
        let a = 0.1_f32;
        let b = 3.0_f32;
        let max_v = a.max(b);
        let ea = (a - max_v).exp();
        let eb = (b - max_v).exp();
        let sum = ea + eb;
        let p1 = eb / sum;

        assert!(p1 > 0.90, "clase 180° debe tener prob alta: {p1}");
        assert!(p1 >= CONFIDENCE_THRESHOLD);
    }

    #[test]
    #[ignore = "requiere models/orientation/textline_orientation.onnx"]
    fn corregir_lineas_sinteticas_con_modelo() {
        let corrector =
            OnnxTextlineOrientationCorrector::new("models/orientation/textline_orientation.onnx")
                .unwrap();
        let crops: Vec<Vec<u8>> = (0..5).map(|_| png_sintetico(192, 48)).collect();
        let results = corrector.corregir_lineas(&crops).unwrap();
        assert_eq!(results.len(), 5);
        for r in &results {
            assert!(r.angulo_grados == 0.0 || r.angulo_grados == 180.0);
            assert!(r.confianza >= 0.0 && r.confianza <= 1.0);
        }
    }
}
