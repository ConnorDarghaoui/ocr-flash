// =============================================================================
// OnnxOrientationCorrector — Implementación de Inferencia de Orientación
//
// Propósito: Satisfacer el requerimiento de corrección automática (ADR-008)
//            utilizando aceleración por hardware vía Microsoft ONNX Runtime (`ort`).
//            Encapsula la complejidad de manipulación de tensores N-dimensionales 
//            y normalización de color (ImageNet).
// =============================================================================

use std::io::Cursor;
use std::sync::Mutex;

use image::{DynamicImage, ImageFormat, imageops};
use ndarray::Array4;
use ort::session::Session;
use ort::value::TensorRef;
use reconstructor_domain::traits::{OrientationCorrector, OrientationResult};
use reconstructor_domain::DomainError;

use crate::error::InfraError;
use crate::ort_session::construir_sesion;

const INPUT_SIZE: u32 = 224;
const CONFIDENCE_THRESHOLD: f32 = 0.85;

const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

/// Adaptador del dominio para el motor ONNX.
///
/// La sesión de `ort` se encuentra encapsulada en un Mutex para permitir su
/// instanciación estática en el "Composition Root" y su posterior uso concurrente
/// sin romper las reglas de borrowing del borrow checker.
pub struct OnnxOrientationCorrector {
    session: Mutex<Session>,
}

impl OnnxOrientationCorrector {
    pub fn new(model_path: &str) -> Result<Self, InfraError> {
        Self::new_with_gpu(model_path, false)
    }

    /// Inicializa la red neuronal intentando acoplarse al driver gráfico subyacente.
    ///
    /// # Arguments
    ///
    /// * `model_path` - Ruta determinista al binario `.onnx` descargado.
    /// * `use_gpu` - Activa Execution Providers (CUDA, TensorRT) delegando en `ort_session`.
    pub fn new_with_gpu(model_path: &str, use_gpu: bool) -> Result<Self, InfraError> {
        let session = construir_sesion(model_path, use_gpu)
            .map_err(|e| InfraError::Onnx(e.to_string()))?;
        Ok(Self { session: Mutex::new(session) })
    }

    fn preprocesar(img: &DynamicImage) -> Array4<f32> {
        let (w, h) = (img.width(), img.height());
        let (new_w, new_h) = if w <= h {
            (INPUT_SIZE, h * INPUT_SIZE / w)
        } else {
            (w * INPUT_SIZE / h, INPUT_SIZE)
        };

        let resized =
            img.resize_exact(new_w, new_h, image::imageops::FilterType::CatmullRom);
        let x_off = new_w.saturating_sub(INPUT_SIZE) / 2;
        let y_off = new_h.saturating_sub(INPUT_SIZE) / 2;
        let cropped = resized.crop_imm(x_off, y_off, INPUT_SIZE, INPUT_SIZE);
        let rgb = cropped.to_rgb8();

        let s = INPUT_SIZE as usize;
        let mut tensor = Array4::<f32>::zeros([1, 3, s, s]);
        for (y, row) in rgb.rows().enumerate() {
            for (x, pixel) in row.enumerate() {
                let [r, g, b] = pixel.0;
                tensor[[0, 0, y, x]] = (r as f32 / 255.0 - MEAN[0]) / STD[0];
                tensor[[0, 1, y, x]] = (g as f32 / 255.0 - MEAN[1]) / STD[1];
                tensor[[0, 2, y, x]] = (b as f32 / 255.0 - MEAN[2]) / STD[2];
            }
        }
        tensor
    }
}

impl OrientationCorrector for OnnxOrientationCorrector {
    fn corregir_pagina(
        &self,
        imagen_bytes: &[u8],
        _ancho: u32,
        _alto: u32,
    ) -> Result<(Vec<u8>, OrientationResult), DomainError> {
        let img = image::load_from_memory(imagen_bytes)
            .map_err(|e| InfraError::Imagen(e.to_string()))?;

        let tensor = Self::preprocesar(&img);
        let tensor_ref = TensorRef::<f32>::from_array_view(tensor.view())
            .map_err(|e| InfraError::Onnx(e.to_string()))?;

        let mut session = self
            .session
            .lock()
            .map_err(|e| InfraError::Onnx(format!("Mutex poisoned: {e}")))?;

        let outputs = session
            .run(ort::inputs!["x" => tensor_ref])
            .map_err(|e| InfraError::Onnx(e.to_string()))?;

        let result = outputs[0]
            .try_extract_array::<f32>()
            .map_err(|e| InfraError::Onnx(e.to_string()))?;

        let logits_slice = result.as_slice().ok_or_else(|| {
            InfraError::Onnx("Output no contiguo".to_string())
        })?;

        let max_val = logits_slice
            .iter()
            .cloned()
            .fold(f32::NEG_INFINITY, f32::max);
        let exp_sum: f32 = logits_slice.iter().map(|&x| (x - max_val).exp()).sum();
        let probs: Vec<f32> =
            logits_slice.iter().map(|&x| (x - max_val).exp() / exp_sum).collect();

        let (clase, confianza) = probs
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(i, &p)| (i, p))
            .unwrap_or((0, 1.0));

        let angulos = [0.0_f32, 90.0, 180.0, 270.0];
        let angulo = angulos[clase.min(3)];

        if confianza < CONFIDENCE_THRESHOLD || clase == 0 {
            return Ok((
                imagen_bytes.to_vec(),
                OrientationResult {
                    angulo_grados: 0.0,
                    confianza,
                    incierto: confianza < CONFIDENCE_THRESHOLD,
                },
            ));
        }

        let rgba = img.to_rgba8();
        let rotada = match clase {
            1 => DynamicImage::ImageRgba8(imageops::rotate90(&rgba)),
            2 => DynamicImage::ImageRgba8(imageops::rotate180(&rgba)),
            3 => DynamicImage::ImageRgba8(imageops::rotate270(&rgba)),
            _ => img,
        };

        let mut bytes_salida = Vec::new();
        rotada
            .write_to(&mut Cursor::new(&mut bytes_salida), ImageFormat::Png)
            .map_err(|e| InfraError::Imagen(e.to_string()))?;

        Ok((
            bytes_salida,
            OrientationResult { angulo_grados: angulo, confianza, incierto: false },
        ))
    }
}
