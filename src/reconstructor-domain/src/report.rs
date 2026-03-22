// =============================================================================
// report — Generador del Artefacto de Auditoría
//
// Propósito: Compilar las métricas operativas en un reporte humano-legible (F4.6).
//            Se diseña sin dependencias de plantillas (templates) para garantizar 
//            la compilación estática (single binary).
// =============================================================================

use crate::eval::EvalReport;

/// Proyecta las métricas de calidad en un documento auto-contenido estático.
///
/// La generación de un HTML inline sin llamadas a assets externos asegura 
/// que el reporte pueda ser archivado por sistemas de compliance y visualizado 
/// offline sin degradación de formato.
///
/// # Arguments
///
/// * `reportes` - Lista inmutable de evaluaciones a proyectar en el informe.
/// * `titulo` - Etiqueta de cabecera inyectada en el metadato del documento.
/// * `nombres_documentos` - Nomenclatura original de los archivos para trazabilidad cruzada.
pub fn generar_informe_html(
    reportes: &[EvalReport],
    titulo: &str,
    nombres_documentos: &[String],
) -> String {
    let css = CSS_STYLES;
    let resumen = generar_resumen(reportes);
    let tabla_docs = generar_tabla_documentos(reportes, nombres_documentos);
    let targets = generar_tabla_targets(reportes);

    format!(
        r#"<!DOCTYPE html>
<html lang="es">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>{titulo}</title>
  <style>{css}</style>
</head>
<body>
  <div class="container">
    <h1>{titulo}</h1>
    <p class="subtitle">Generado: {fecha}</p>

    <h2>Resumen</h2>
    {resumen}

    <h2>Cumplimiento de Targets SRS §10.3</h2>
    {targets}

    <h2>Métricas por Documento</h2>
    {tabla_docs}

    <footer>
      <p>Reconstructor OCR — Informe de Evaluación Fase 4</p>
    </footer>
  </div>
</body>
</html>"#,
        titulo = escape_html(titulo),
        fecha = timestamp_actual(),
        css = css,
        resumen = resumen,
        targets = targets,
        tabla_docs = tabla_docs,
    )
}

fn generar_resumen(reportes: &[EvalReport]) -> String {
    if reportes.is_empty() {
        return "<p class=\"warn\">Sin datos de evaluación.</p>".into();
    }

    let n = reportes.len() as f64;
    let cer_avg = reportes.iter().map(|r| r.cer_promedio).sum::<f64>() / n;
    let iou_avg = reportes.iter().map(|r| r.layout_iou).sum::<f64>() / n;
    let fr_avg = reportes.iter().map(|r| r.fallback_rate).sum::<f64>() / n;
    let tp_avg = reportes.iter().map(|r| r.throughput).sum::<f64>() / n;
    let total_pags: u32 = reportes.iter().map(|r| r.pipeline.total_paginas).sum();

    let cumple = reportes.iter().filter(|r| r.cumple_targets_srs()).count();
    let estado_global = if cumple == reportes.len() {
        "<span class=\"badge pass\">✓ TODOS LOS TARGETS CUMPLIDOS</span>"
    } else {
        "<span class=\"badge fail\">⚠ ALGUNOS TARGETS INCUMPLIDOS</span>"
    };

    format!(
        r#"<div class="summary-grid">
  <div class="metric-card">
    <div class="metric-value {cer_class}">{cer:.1}%</div>
    <div class="metric-label">CER promedio</div>
    <div class="metric-target">Target: &lt;5%</div>
  </div>
  <div class="metric-card">
    <div class="metric-value {iou_class}">{iou:.3}</div>
    <div class="metric-label">Layout IoU promedio</div>
    <div class="metric-target">Target: &gt;0.85</div>
  </div>
  <div class="metric-card">
    <div class="metric-value {fr_class}">{fr:.1}%</div>
    <div class="metric-label">Fallback Rate promedio</div>
    <div class="metric-target">Target: &lt;15%</div>
  </div>
  <div class="metric-card">
    <div class="metric-value {tp_class}">{tp:.2}</div>
    <div class="metric-label">Throughput pág/seg</div>
    <div class="metric-target">Target: ≥2.0</div>
  </div>
  <div class="metric-card">
    <div class="metric-value">{docs}</div>
    <div class="metric-label">Documentos evaluados</div>
  </div>
  <div class="metric-card">
    <div class="metric-value">{pags}</div>
    <div class="metric-label">Páginas totales</div>
  </div>
</div>
<p>{estado}</p>"#,
        cer = cer_avg * 100.0,
        cer_class = if cer_avg < 0.05 { "pass" } else { "fail" },
        iou = iou_avg,
        iou_class = if iou_avg > 0.85 { "pass" } else { "fail" },
        fr = fr_avg * 100.0,
        fr_class = if fr_avg < 0.15 { "pass" } else { "fail" },
        tp = tp_avg,
        tp_class = if tp_avg >= 2.0 { "pass" } else { "fail" },
        docs = reportes.len(),
        pags = total_pags,
        estado = estado_global,
    )
}

fn generar_tabla_targets(reportes: &[EvalReport]) -> String {
    let n = reportes.len() as f64;
    if n == 0.0 {
        return "<p>Sin datos.</p>".into();
    }

    let cer_avg = reportes.iter().map(|r| r.cer_promedio).sum::<f64>() / n;
    let iou_avg = reportes.iter().map(|r| r.layout_iou).sum::<f64>() / n;
    let fr_avg = reportes.iter().map(|r| r.fallback_rate).sum::<f64>() / n;
    let tp_avg = reportes.iter().map(|r| r.throughput).sum::<f64>() / n;

    let filas = [
        ("CER (Character Error Rate)", "<5%", format!("{:.2}%", cer_avg * 100.0), cer_avg < 0.05),
        ("Layout IoU", ">0.85", format!("{iou_avg:.3}"), iou_avg > 0.85),
        ("Fallback Rate", "<15%", format!("{:.1}%", fr_avg * 100.0), fr_avg < 0.15),
        ("Throughput (pág/seg)", "≥2.0", format!("{tp_avg:.2}"), tp_avg >= 2.0),
    ];

    let filas_html: String = filas
        .iter()
        .map(|(metrica, target, valor, cumple)| {
            let icono = if *cumple { "✓" } else { "✗" };
            let clase = if *cumple { "pass" } else { "fail" };
            format!(
                "<tr><td>{metrica}</td><td>{target}</td><td><strong>{valor}</strong></td>\
                 <td class=\"{clase}\">{icono}</td></tr>"
            )
        })
        .collect();

    format!(
        r#"<table>
  <thead><tr><th>Métrica</th><th>Target SRS</th><th>Valor obtenido</th><th>Estado</th></tr></thead>
  <tbody>{filas_html}</tbody>
</table>"#
    )
}

fn generar_tabla_documentos(reportes: &[EvalReport], nombres: &[String]) -> String {
    if reportes.is_empty() {
        return "<p>Sin documentos evaluados.</p>".into();
    }

    let filas: String = reportes
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let nombre = nombres.get(i).map(|s| s.as_str()).unwrap_or("—");
            let cumple = if r.cumple_targets_srs() {
                "<span class=\"badge pass\">✓ OK</span>"
            } else {
                "<span class=\"badge fail\">✗ NOK</span>"
            };
            format!(
                "<tr>\
                  <td>{nombre}</td>\
                  <td>{pags}</td>\
                  <td class=\"{cer_c}\">{cer:.2}%</td>\
                  <td class=\"{iou_c}\">{iou:.3}</td>\
                  <td class=\"{fr_c}\">{fr:.1}%</td>\
                  <td class=\"{tp_c}\">{tp:.2}</td>\
                  <td>{cumple}</td>\
                </tr>",
                nombre = escape_html(nombre),
                pags = r.pipeline.total_paginas,
                cer = r.cer_promedio * 100.0,
                cer_c = if r.cer_promedio < 0.05 { "pass" } else { "fail" },
                iou = r.layout_iou,
                iou_c = if r.layout_iou > 0.85 { "pass" } else { "fail" },
                fr = r.fallback_rate * 100.0,
                fr_c = if r.fallback_rate < 0.15 { "pass" } else { "fail" },
                tp = r.throughput,
                tp_c = if r.throughput >= 2.0 { "pass" } else { "fail" },
                cumple = cumple,
            )
        })
        .collect();

    format!(
        r#"<table>
  <thead>
    <tr>
      <th>Documento</th><th>Páginas</th><th>CER</th>
      <th>Layout IoU</th><th>Fallback</th><th>Pág/seg</th><th>Estado</th>
    </tr>
  </thead>
  <tbody>{filas}</tbody>
</table>"#
    )
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn timestamp_actual() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("Unix epoch +{secs}s (ejecutar date -d @{secs} para convertir)")
}

const CSS_STYLES: &str = r#"
* { box-sizing: border-box; margin: 0; padding: 0; }
body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
       background: #f5f5f5; color: #333; line-height: 1.6; }
.container { max-width: 1100px; margin: 0 auto; padding: 2rem; }
h1 { font-size: 1.8rem; margin-bottom: 0.25rem; }
h2 { font-size: 1.3rem; margin: 2rem 0 0.75rem; border-bottom: 2px solid #ddd; padding-bottom: 0.25rem; }
.subtitle { color: #666; font-size: 0.9rem; margin-bottom: 1.5rem; }
.summary-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(160px, 1fr)); gap: 1rem; margin-bottom: 1rem; }
.metric-card { background: #fff; border-radius: 8px; padding: 1rem; text-align: center; box-shadow: 0 1px 4px rgba(0,0,0,.1); }
.metric-value { font-size: 1.6rem; font-weight: 700; }
.metric-label { font-size: 0.8rem; color: #555; margin-top: 0.2rem; }
.metric-target { font-size: 0.75rem; color: #888; }
table { width: 100%; border-collapse: collapse; background: #fff; border-radius: 8px; overflow: hidden; box-shadow: 0 1px 4px rgba(0,0,0,.1); }
th, td { padding: 0.6rem 0.9rem; text-align: left; border-bottom: 1px solid #eee; }
th { background: #f0f0f0; font-size: 0.85rem; font-weight: 600; }
tr:last-child td { border-bottom: none; }
.pass { color: #2d9e5a; }
.fail { color: #c0392b; }
.warn { color: #e67e22; }
.badge { font-size: 0.8rem; padding: 0.15rem 0.5rem; border-radius: 4px; font-weight: 600; }
.badge.pass { background: #e8f8ef; }
.badge.fail { background: #fdecea; }
footer { margin-top: 3rem; text-align: center; font-size: 0.8rem; color: #aaa; }
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{eval::EvalReport, ProcessingMetrics};

    fn reporte_ejemplo(cer: f64, iou: f64, fallback: u32, total: u32, ms: f64, pags: u32) -> EvalReport {
        let m = ProcessingMetrics {
            total_paginas: pags,
            total_bloques_detectados: total,
            bloques_fallback_raster: fallback,
            tiempo_total_ms: ms,
            tiempo_promedio_por_pagina_ms: if pags > 0 { ms / pags as f64 } else { 0.0 },
            ..Default::default()
        };
        EvalReport::new(m, cer, iou)
    }

    #[test]
    fn html_generado_contiene_titulo() {
        let r = reporte_ejemplo(0.02, 0.92, 2, 20, 3000.0, 5);
        let html = generar_informe_html(&[r], "Test Informe", &["doc1.pdf".into()]);
        assert!(html.contains("Test Informe"), "debe contener el título");
    }

    #[test]
    fn html_generado_es_html_valido_estructura() {
        let r = reporte_ejemplo(0.03, 0.88, 3, 30, 5000.0, 10);
        let html = generar_informe_html(&[r], "Informe", &["test.pdf".into()]);
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("</html>"));
        assert!(html.contains("<table>"));
    }

    #[test]
    fn html_sin_reportes_no_panics() {
        let html = generar_informe_html(&[], "Vacío", &[]);
        assert!(html.contains("Sin datos"));
    }

    #[test]
    fn html_multiples_documentos() {
        let reportes = vec![
            reporte_ejemplo(0.02, 0.92, 1, 10, 2000.0, 4),
            reporte_ejemplo(0.08, 0.75, 5, 20, 6000.0, 3), // falla targets
        ];
        let nombres = vec!["doc_limpio.pdf".into(), "doc_ruidoso.pdf".into()];
        let html = generar_informe_html(&reportes, "Multi-doc", &nombres);
        assert!(html.contains("doc_limpio.pdf"));
        assert!(html.contains("doc_ruidoso.pdf"));
        assert!(html.contains("✓ OK"));
        assert!(html.contains("✗ NOK"));
    }

    #[test]
    fn escape_html_escapa_caracteres() {
        assert_eq!(escape_html("<b>test & 'x'</b>"), "&lt;b&gt;test &amp; 'x'&lt;/b&gt;");
    }
}
