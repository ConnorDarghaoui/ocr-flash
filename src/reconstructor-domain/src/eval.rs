// =============================================================================
// eval — Heurísticas de Aseguramiento de Calidad (F4.2)
//
// Propósito: Desacopla la lógica de validación de los KPIs del negocio.
//            Las funciones puras (sin I/O) previenen cuellos de botella 
//            durante la recolección de telemetría y aseguran testeabilidad aislada.
// =============================================================================

use crate::{BoundingBox, ProcessingMetrics};

// ─── CER ─────────────────────────────────────────────────────────────────────

/// Determina la fidelidad del texto inferido frente al corpus real (Ground Truth).
///
/// La métrica de Distancia de Levenshtein (CER) permite evaluar el desempeño del 
/// pipeline OCR al detectar falsos positivos (inserciones) y omisiones (eliminaciones), 
/// validando si la calidad cumple con los márgenes del negocio (SRS §10.3).
///
/// # Arguments
///
/// * `predicted` - Cadena de texto generada por el bloque de inferencia (e.g. PaddleOCR).
/// * `ground_truth` - Cadena de texto de validación perfecta.
///
/// # Returns
///
/// Ratio de error `[0.0, ∞)`. Valores sobre 1.0 implican severas inserciones anómalas.
pub fn cer(predicted: &str, ground_truth: &str) -> f64 {
    let gt_chars: Vec<char> = ground_truth.chars().collect();
    let pred_chars: Vec<char> = predicted.chars().collect();

    if gt_chars.is_empty() {
        return if pred_chars.is_empty() { 0.0 } else { 1.0 };
    }

    let dist = edit_distance(&pred_chars, &gt_chars);
    dist as f64 / gt_chars.len() as f64
}

fn edit_distance(a: &[char], b: &[char]) -> usize {
    let m = a.len();
    let n = b.len();

    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0usize; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1)
                .min(curr[j - 1] + 1)
                .min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
}

// ─── Layout IoU ──────────────────────────────────────────────────────────────

/// Audita la desviación geométrica producida por la red de Layout (YOLO).
pub fn iou(a: &BoundingBox, b: &BoundingBox) -> f64 {
    let ix1 = a.x.max(b.x);
    let iy1 = a.y.max(b.y);
    let ix2 = (a.x + a.width).min(b.x + b.width);
    let iy2 = (a.y + a.height).min(b.y + b.height);

    if ix2 <= ix1 || iy2 <= iy1 {
        return 0.0;
    }

    let inter = (ix2 - ix1) as f64 * (iy2 - iy1) as f64;
    let area_a = a.width as f64 * a.height as f64;
    let area_b = b.width as f64 * b.height as f64;
    let union = area_a + area_b - inter;

    if union <= 0.0 { 0.0 } else { inter / union }
}

/// Condensa el rendimiento espacial de toda la capa gráfica en una métrica escalar.
pub fn layout_iou(predicted: &[BoundingBox], ground_truth: &[BoundingBox]) -> f64 {
    if predicted.is_empty() || ground_truth.is_empty() {
        return 0.0;
    }

    let sum: f64 = predicted
        .iter()
        .map(|pred| {
            ground_truth
                .iter()
                .map(|gt| iou(pred, gt))
                .fold(0.0_f64, f64::max)
        })
        .sum();

    sum / predicted.len() as f64
}

// ─── Fallback Rate ────────────────────────────────────────────────────────────

/// Cuantifica el ratio de degradación del pipeline. Valores altos evidencian fallas de inferencia.
pub fn fallback_rate(metrics: &ProcessingMetrics) -> f64 {
    if metrics.total_bloques_detectados == 0 {
        return 0.0;
    }
    metrics.bloques_fallback_raster as f64 / metrics.total_bloques_detectados as f64
}

/// Valida el requerimiento no funcional de latencia sistémica (Hardware-bound).
pub fn throughput_paginas_por_segundo(metrics: &ProcessingMetrics) -> f64 {
    if metrics.tiempo_total_ms <= 0.0 {
        return 0.0;
    }
    metrics.total_paginas as f64 / (metrics.tiempo_total_ms / 1000.0)
}

// ─── EvalReport ──────────────────────────────────────────────────────────────

/// Reporte consolidado de la auditoría.
#[derive(Debug, Clone)]
pub struct EvalReport {
    pub cer_promedio: f64,
    pub layout_iou: f64,
    pub fallback_rate: f64,
    pub throughput: f64,
    pub pipeline: ProcessingMetrics,
}

impl EvalReport {
    pub fn new(
        metrics: ProcessingMetrics,
        cer: f64,
        layout_iou_val: f64,
    ) -> Self {
        let fr = fallback_rate(&metrics);
        let tp = throughput_paginas_por_segundo(&metrics);
        Self {
            cer_promedio: cer,
            layout_iou: layout_iou_val,
            fallback_rate: fr,
            throughput: tp,
            pipeline: metrics,
        }
    }

    /// Comprueba contractualmente la viabilidad operativa estipulada en el SRS.
    pub fn cumple_targets_srs(&self) -> bool {
        self.cer_promedio < 0.05
            && self.layout_iou > 0.85
            && self.fallback_rate < 0.15
            && self.throughput >= 2.0
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cer_textos_identicos() {
        assert_eq!(cer("hola mundo", "hola mundo"), 0.0);
    }

    #[test]
    fn cer_predicho_vacio_gt_no_vacio() {
        let gt = "abc";
        assert_eq!(cer("", gt), 1.0);
    }

    #[test]
    fn cer_ambos_vacios() {
        assert_eq!(cer("", ""), 0.0);
    }

    #[test]
    fn cer_un_caracter_diferente() {
        let c = cer("gola mundo", "hola mundo");
        assert!((c - 0.1).abs() < 1e-9, "esperado 0.1, obtenido {c}");
    }

    #[test]
    fn cer_inserciones_extra() {
        let c = cer("abcXd", "abcd");
        assert!((c - 0.25).abs() < 1e-9, "esperado 0.25, obtenido {c}");
    }

    #[test]
    fn cer_puede_superar_uno() {
        let c = cer("xxxxx", "a");
        assert!(c > 1.0, "CER debe poder superar 1.0 con muchas inserciones");
    }

    fn bbox(x: f32, y: f32, w: f32, h: f32) -> BoundingBox {
        BoundingBox { x, y, width: w, height: h }
    }

    #[test]
    fn iou_misma_bbox() {
        let b = bbox(0.0, 0.0, 100.0, 100.0);
        let resultado = iou(&b, &b);
        assert!((resultado - 1.0).abs() < 1e-6, "IoU idénticas debe ser 1.0");
    }

    #[test]
    fn iou_sin_solapamiento() {
        let a = bbox(0.0, 0.0, 50.0, 50.0);
        let b = bbox(100.0, 100.0, 50.0, 50.0);
        assert_eq!(iou(&a, &b), 0.0);
    }

    #[test]
    fn iou_solapamiento_parcial() {
        let a = bbox(0.0, 0.0, 100.0, 100.0);
        let b = bbox(50.0, 50.0, 100.0, 100.0);
        let resultado = iou(&a, &b);
        let esperado = 2500.0 / 17500.0;
        assert!((resultado - esperado).abs() < 1e-6, "IoU={resultado}, esperado={esperado}");
    }

    #[test]
    fn iou_b_dentro_de_a() {
        let a = bbox(0.0, 0.0, 100.0, 100.0);
        let b = bbox(25.0, 25.0, 50.0, 50.0);
        let resultado = iou(&a, &b);
        let esperado = 2500.0 / 10000.0;
        assert!((resultado - esperado).abs() < 1e-6, "IoU={resultado}, esperado={esperado}");
    }

    #[test]
    fn layout_iou_prediccion_perfecta() {
        let bboxes = vec![bbox(0.0, 0.0, 100.0, 100.0), bbox(200.0, 0.0, 100.0, 100.0)];
        let resultado = layout_iou(&bboxes, &bboxes);
        assert!((resultado - 1.0).abs() < 1e-6);
    }

    #[test]
    fn layout_iou_sin_solapamiento() {
        let pred = vec![bbox(0.0, 0.0, 50.0, 50.0)];
        let gt = vec![bbox(100.0, 100.0, 50.0, 50.0)];
        assert_eq!(layout_iou(&pred, &gt), 0.0);
    }

    #[test]
    fn layout_iou_listas_vacias() {
        let empty: Vec<BoundingBox> = vec![];
        assert_eq!(layout_iou(&empty, &[bbox(0.0, 0.0, 100.0, 100.0)]), 0.0);
        assert_eq!(layout_iou(&[bbox(0.0, 0.0, 100.0, 100.0)], &empty), 0.0);
    }

    #[test]
    fn fallback_rate_sin_bloques() {
        let m = ProcessingMetrics::default();
        assert_eq!(fallback_rate(&m), 0.0);
    }

    #[test]
    fn fallback_rate_todos_fallback() {
        let m = ProcessingMetrics {
            total_bloques_detectados: 10,
            bloques_fallback_raster: 10,
            ..Default::default()
        };
        assert!((fallback_rate(&m) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn fallback_rate_mitad() {
        let m = ProcessingMetrics {
            total_bloques_detectados: 8,
            bloques_fallback_raster: 4,
            ..Default::default()
        };
        assert!((fallback_rate(&m) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn throughput_dos_paginas_un_segundo() {
        let m = ProcessingMetrics {
            total_paginas: 2,
            tiempo_total_ms: 1000.0,
            ..Default::default()
        };
        assert!((throughput_paginas_por_segundo(&m) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn throughput_tiempo_cero() {
        let m = ProcessingMetrics { total_paginas: 5, ..Default::default() };
        assert_eq!(throughput_paginas_por_segundo(&m), 0.0);
    }

    #[test]
    fn eval_report_cumple_targets_buenos() {
        let m = ProcessingMetrics {
            total_paginas: 10,
            total_bloques_detectados: 100,
            bloques_fallback_raster: 5,
            tiempo_total_ms: 4000.0,
            ..Default::default()
        };
        let report = EvalReport::new(m, 0.02, 0.90);
        assert!(report.cumple_targets_srs(), "debe cumplir todos los targets");
    }

    #[test]
    fn eval_report_no_cumple_cer_alto() {
        let m = ProcessingMetrics {
            total_paginas: 10,
            total_bloques_detectados: 100,
            bloques_fallback_raster: 5,
            tiempo_total_ms: 4000.0,
            ..Default::default()
        };
        let report = EvalReport::new(m, 0.08, 0.90);
        assert!(!report.cumple_targets_srs());
    }
}
