// =============================================================================
// DocLayoutYoloDetector — Segmentación Espacial Acelerada
//
// Propósito: Desacoplar la inferencia de topología visual (YOLOv10) del orquestador.
//            Previene solapamientos fantasma (vía NMS) y estructura el reading-order 
//            en una sola pasada síncrona antes de despachar a la granja de OCR.
// =============================================================================

use std::sync::Mutex;

use image::{DynamicImage, Rgba, RgbaImage};
use ndarray::Array4;
use ort::session::Session;
use ort::value::TensorRef;
use reconstructor_domain::traits::LayoutDetector;
use reconstructor_domain::{BoundingBox, BlockType, DomainError, Region};

use crate::error::InfraError;
use crate::ort_session::construir_sesion;

const MODEL_INPUT_SIZE: u32 = 1024;
const CONF_THRESHOLD: f32 = 0.50;
const NMS_IOU_THRESHOLD: f32 = 0.45;

/// Interfaz segura (Mutex) para la ejecución concurrente del modelo DocLayout-YOLO.
pub struct DocLayoutYoloDetector {
    session: Mutex<Session>,
}

impl DocLayoutYoloDetector {
    pub fn new(model_path: &str) -> Result<Self, InfraError> {
        Self::new_with_gpu(model_path, false)
    }

    /// Hidrata el grafo de inferencia en el backend solicitado (CPU/CUDA).
    pub fn new_with_gpu(model_path: &str, use_gpu: bool) -> Result<Self, InfraError> {
        let session = construir_sesion(model_path, use_gpu)
            .map_err(|e| InfraError::Onnx(e.to_string()))?;
        Ok(Self { session: Mutex::new(session) })
    }
}

impl LayoutDetector for DocLayoutYoloDetector {
    /// Infiere iterativamente regiones aisladas aplicando un filtro de solapamiento.
    ///
    /// Transforma el tensor decodificado de YOLO `[1, N, 6]` a la estructura canónica `Region`.
    /// El método garantiza que el array resultante cumpla con el flujo natural de lectura humana 
    /// (Top-to-Bottom, Left-to-Right) ordenando heurísticamente los bounding boxes.
    ///
    /// # Arguments
    ///
    /// * `imagen_bytes` - Buffer de píxeles sin comprimir.
    /// * `ancho` / `alto` - Dimensiones físicas provistas por el pipeline superior.
    /// * `numero_pagina` - Semilla ordinal inyectada para el rastreo de auditoría.
    ///
    /// # Errors
    ///
    /// Lanza `DomainError` ante fallos catastróficos del grafo `ort` o OOM.
    fn detectar(
        &self,
        imagen_bytes: &[u8],
        ancho: u32,
        alto: u32,
        numero_pagina: u32,
    ) -> Result<Vec<Region>, DomainError> {
        let img = image::load_from_memory(imagen_bytes)
            .map_err(|e| InfraError::Imagen(e.to_string()))?;

        let (tensor, scale, pad_x, pad_y) = letterbox(&img, MODEL_INPUT_SIZE);

        let tensor_ref = TensorRef::<f32>::from_array_view(tensor.view())
            .map_err(|e| InfraError::Onnx(e.to_string()))?;

        let mut session = self
            .session
            .lock()
            .map_err(|e| InfraError::Onnx(format!("Mutex poisoned: {e}")))?;

        let outputs = session
            .run(ort::inputs!["images" => tensor_ref])
            .map_err(|e| InfraError::Onnx(e.to_string()))?;

        let out_array = outputs[0]
            .try_extract_array::<f32>()
            .map_err(|e| InfraError::Onnx(e.to_string()))?;

        let shape = out_array.shape().to_vec();
        let num_boxes = if shape.len() >= 2 { shape[1] } else { 0 };

        let data = out_array
            .as_slice()
            .ok_or_else(|| InfraError::Onnx("Output no contiguo".to_string()))?;

        let mut detecciones: Vec<(BoundingBox, f32, usize)> = Vec::new();

        for i in 0..num_boxes {
            let base = i * 6;
            if base + 5 >= data.len() {
                break;
            }
            let conf = data[base + 4];
            if conf < CONF_THRESHOLD {
                continue;
            }
            let x1 = data[base];
            let y1 = data[base + 1];
            let x2 = data[base + 2];
            let y2 = data[base + 3];
            let clase = data[base + 5] as usize;

            let x1_orig = ((x1 - pad_x) / scale).max(0.0).min(ancho as f32);
            let y1_orig = ((y1 - pad_y) / scale).max(0.0).min(alto as f32);
            let x2_orig = ((x2 - pad_x) / scale).max(0.0).min(ancho as f32);
            let y2_orig = ((y2 - pad_y) / scale).max(0.0).min(alto as f32);

            let bbox = BoundingBox::new(
                x1_orig,
                y1_orig,
                (x2_orig - x1_orig).max(0.0),
                (y2_orig - y1_orig).max(0.0),
            );
            detecciones.push((bbox, conf, clase));
        }

        let mut mantenidas = nms(detecciones, NMS_IOU_THRESHOLD);

        mantenidas.sort_by(|a, b| {
            let ay = a.0.y as i32;
            let by_val = b.0.y as i32;
            if (ay - by_val).abs() > 20 {
                ay.cmp(&by_val)
            } else {
                (a.0.x as i32).cmp(&(b.0.x as i32))
            }
        });

        let regiones = mantenidas
            .into_iter()
            .enumerate()
            .map(|(idx, (bbox, conf, clase))| {
                Region::new(
                    format!("blk_{numero_pagina}_{idx}"),
                    clase_a_block_type(clase),
                    bbox,
                    conf,
                )
            })
            .collect();

        Ok(regiones)
    }
}

fn letterbox(img: &DynamicImage, target: u32) -> (Array4<f32>, f32, f32, f32) {
    let (orig_w, orig_h) = (img.width(), img.height());
    let scale =
        (target as f32 / orig_w as f32).min(target as f32 / orig_h as f32);
    let new_w = (orig_w as f32 * scale).round() as u32;
    let new_h = (orig_h as f32 * scale).round() as u32;

    let resized =
        img.resize_exact(new_w, new_h, image::imageops::FilterType::CatmullRom);
    let mut padded =
        RgbaImage::from_pixel(target, target, Rgba([128, 128, 128, 255]));

    let pad_x = (target - new_w) / 2;
    let pad_y = (target - new_h) / 2;
    image::imageops::overlay(
        &mut padded,
        &resized.to_rgba8(),
        pad_x as i64,
        pad_y as i64,
    );

    let t = target as usize;
    let mut tensor = Array4::<f32>::zeros([1, 3, t, t]);
    for (y, row) in padded.rows().enumerate() {
        for (x, pixel) in row.enumerate() {
            tensor[[0, 0, y, x]] = pixel.0[0] as f32 / 255.0;
            tensor[[0, 1, y, x]] = pixel.0[1] as f32 / 255.0;
            tensor[[0, 2, y, x]] = pixel.0[2] as f32 / 255.0;
        }
    }

    (tensor, scale, pad_x as f32, pad_y as f32)
}

fn nms(
    mut detecciones: Vec<(BoundingBox, f32, usize)>,
    iou_threshold: f32,
) -> Vec<(BoundingBox, f32, usize)> {
    detecciones.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let mut suprimido = vec![false; detecciones.len()];
    let mut resultado = Vec::new();

    for i in 0..detecciones.len() {
        if suprimido[i] {
            continue;
        }
        resultado.push(detecciones[i].clone());
        for j in (i + 1)..detecciones.len() {
            if detecciones[i].0.iou(&detecciones[j].0) > iou_threshold {
                suprimido[j] = true;
            }
        }
    }
    resultado
}

fn clase_a_block_type(clase: usize) -> BlockType {
    match clase {
        0 => BlockType::Caption,
        1 => BlockType::Footer,
        2 => BlockType::Formula,
        3 => BlockType::List,
        4 => BlockType::Footer,
        5 => BlockType::Header,
        6 => BlockType::Figure,
        7 | 10 => BlockType::Title,
        8 => BlockType::Table,
        9 => BlockType::Text,
        _ => BlockType::Unknown,
    }
}
