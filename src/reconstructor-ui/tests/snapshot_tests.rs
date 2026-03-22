// =============================================================================
// snapshot_tests — Tests de regresión con golden files (F4.5)
//
// Estrategia:
//   - Procesar fixtures PNG sintéticas con el pipeline fallback-only
//   - Comparar el JSON de salida contra golden files en tests/fixtures/golden/
//   - Si UPDATE_SNAPSHOTS=1, regenerar los golden files
//
// Ejecución normal:    cargo test -p reconstructor-ui --test snapshot_tests
// Actualizar goldens:  UPDATE_SNAPSHOTS=1 cargo test -p reconstructor-ui --test snapshot_tests
// =============================================================================

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use image::{DynamicImage, ImageFormat, RgbImage};
use reconstructor_app::PipelineOrchestrator;
use reconstructor_domain::{
    BoundingBox, BlockType, DomainError, LayoutDetector, OrientationCorrector, OrientationResult,
    PageImage, PipelineConfig, Region, ResolverFactory,
};
use reconstructor_infra::{
    JsonOutputGenerator, PrintPdfComposer, RasterFallbackResolver,
};

// ─── Stubs ───────────────────────────────────────────────────────────────────

struct NoopOrientation;
impl OrientationCorrector for NoopOrientation {
    fn corregir_pagina(
        &self,
        b: &[u8],
        _: u32,
        _: u32,
    ) -> Result<(Vec<u8>, OrientationResult), DomainError> {
        Ok((b.to_vec(), OrientationResult { angulo_grados: 0.0, confianza: 1.0, incierto: false }))
    }
}

/// Layout que divide la página en N regiones horizontales iguales.
struct NRegionesLayout(u32);
impl LayoutDetector for NRegionesLayout {
    fn detectar(
        &self,
        _: &[u8],
        ancho: u32,
        alto: u32,
        num_pagina: u32,
    ) -> Result<Vec<Region>, DomainError> {
        let n = self.0;
        let alto_region = (alto as f32) / (n as f32);
        let regiones = (0..n)
            .map(|i| Region {
                id: format!("blk_{num_pagina}_{i}"),
                tipo_bloque: BlockType::Text,
                bbox: BoundingBox {
                    x: 0.0,
                    y: i as f32 * alto_region,
                    width: ancho as f32,
                    height: alto_region,
                },
                confianza_deteccion: 0.95,
            })
            .collect();
        Ok(regiones)
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn crear_png(ancho: u32, alto: u32) -> Vec<u8> {
    let img = DynamicImage::ImageRgb8(RgbImage::new(ancho, alto));
    let mut bytes = Vec::new();
    img.write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png).unwrap();
    bytes
}

fn golden_dir() -> PathBuf {
    // Relativo al crate root durante tests
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/golden")
}

fn output_dir() -> PathBuf {
    std::env::temp_dir().join("reconstructor_snapshot_tests")
}

fn procesar_fixture(
    nombre: &str,
    paginas: Vec<PageImage>,
    n_regiones_por_pagina: u32,
) -> serde_json::Value {
    let config = PipelineConfig::default();
    let out_dir = output_dir().join(nombre);
    std::fs::create_dir_all(&out_dir).unwrap();
    let ruta = out_dir.join("output").to_string_lossy().to_string();

    let (orch, _rx) = PipelineOrchestrator::new(
        Box::new(NoopOrientation),
        Box::new(NRegionesLayout(n_regiones_por_pagina)),
        ResolverFactory::new(vec![Box::new(RasterFallbackResolver)]),
        Box::new(PrintPdfComposer),
        vec![Box::new(JsonOutputGenerator)],
        config,
    );

    orch.procesar(paginas, &ruta).expect("pipeline debe procesar sin errores");

    let json_ruta = format!("{ruta}.json");
    let json_str = std::fs::read_to_string(&json_ruta)
        .unwrap_or_else(|_| panic!("JSON de salida no encontrado en {json_ruta}"));

    let mut value: serde_json::Value = serde_json::from_str(&json_str).unwrap();

    // Normalizar campos no-deterministas para comparación estable:
    // - procesado_en (timestamp)
    // - tiempo_procesamiento_ms (varía por hardware)
    // - tiempo_total_ms, tiempo_promedio_por_pagina_ms
    normalizar_para_snapshot(&mut value);
    value
}

fn normalizar_para_snapshot(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(map) => {
            for key in &["procesado_en", "tiempo_procesamiento_ms", "tiempo_total_ms",
                         "tiempo_promedio_por_pagina_ms"] {
                if map.contains_key(*key) {
                    map.insert(key.to_string(), serde_json::Value::String("<normalizado>".into()));
                }
            }
            // También normalizar imagen_bytes (datos binarios, no deterministas en printout)
            if map.contains_key("imagen_bytes") {
                map.insert("imagen_bytes".to_string(), serde_json::Value::String("<bytes>".into()));
            }
            for val in map.values_mut() {
                normalizar_para_snapshot(val);
            }
        }
        serde_json::Value::Array(arr) => {
            for val in arr.iter_mut() {
                normalizar_para_snapshot(val);
            }
        }
        _ => {}
    }
}

fn comparar_o_actualizar(nombre: &str, actual: &serde_json::Value) {
    let golden_path = golden_dir().join(format!("{nombre}.json"));
    let update_mode = std::env::var("UPDATE_SNAPSHOTS").map(|v| v == "1").unwrap_or(false);

    if update_mode {
        std::fs::create_dir_all(golden_dir()).unwrap();
        let contenido = serde_json::to_string_pretty(actual).unwrap();
        std::fs::write(&golden_path, contenido).unwrap();
        println!("Golden actualizado: {}", golden_path.display());
        return;
    }

    if !golden_path.exists() {
        // Primera ejecución: crear el golden automáticamente
        std::fs::create_dir_all(golden_dir()).unwrap();
        let contenido = serde_json::to_string_pretty(actual).unwrap();
        std::fs::write(&golden_path, &contenido).unwrap();
        println!("Golden creado: {}", golden_path.display());
        return;
    }

    let golden_str = std::fs::read_to_string(&golden_path).unwrap();
    let golden: serde_json::Value = serde_json::from_str(&golden_str).unwrap();

    assert_eq!(
        actual,
        &golden,
        "Regresión detectada en snapshot '{nombre}'.\n\
         Para actualizar: UPDATE_SNAPSHOTS=1 cargo test -p reconstructor-ui --test snapshot_tests"
    );
}

// ─── Fixtures ────────────────────────────────────────────────────────────────

/// Fixture 1: Página en blanco, 1 región
#[test]
fn snapshot_pagina_blanca_1_region() {
    let paginas = vec![PageImage {
        datos: crear_png(595, 842),
        ancho: 595,
        alto: 842,
        numero_pagina: 1,
    }];
    let resultado = procesar_fixture("pagina_blanca_1_region", paginas, 1);
    comparar_o_actualizar("pagina_blanca_1_region", &resultado);
}

/// Fixture 2: Página en blanco, 3 regiones
#[test]
fn snapshot_pagina_blanca_3_regiones() {
    let paginas = vec![PageImage {
        datos: crear_png(595, 842),
        ancho: 595,
        alto: 842,
        numero_pagina: 1,
    }];
    let resultado = procesar_fixture("pagina_blanca_3_regiones", paginas, 3);
    comparar_o_actualizar("pagina_blanca_3_regiones", &resultado);
}

/// Fixture 3: Documento de 3 páginas, 2 regiones por página
#[test]
fn snapshot_3_paginas_2_regiones_cada_una() {
    let paginas: Vec<PageImage> = (1..=3)
        .map(|i| PageImage {
            datos: crear_png(595, 842),
            ancho: 595,
            alto: 842,
            numero_pagina: i,
        })
        .collect();
    let resultado = procesar_fixture("3_paginas_2_regiones", paginas, 2);
    comparar_o_actualizar("3_paginas_2_regiones", &resultado);
}

/// Fixture 4: Imagen pequeña (thumbnail-size), 1 región
#[test]
fn snapshot_imagen_pequena() {
    let paginas = vec![PageImage {
        datos: crear_png(100, 100),
        ancho: 100,
        alto: 100,
        numero_pagina: 1,
    }];
    let resultado = procesar_fixture("imagen_pequena_100x100", paginas, 1);
    comparar_o_actualizar("imagen_pequena_100x100", &resultado);
}

/// Fixture 5: Página apaisada (landscape A4), 4 regiones
#[test]
fn snapshot_pagina_apaisada_4_regiones() {
    let paginas = vec![PageImage {
        datos: crear_png(842, 595),
        ancho: 842,
        alto: 595,
        numero_pagina: 1,
    }];
    let resultado = procesar_fixture("pagina_apaisada_4_regiones", paginas, 4);
    comparar_o_actualizar("pagina_apaisada_4_regiones", &resultado);
}

// ─── Test de integridad de goldens ───────────────────────────────────────────

/// Verifica que todos los golden files son JSON válido y tienen la estructura esperada.
#[test]
fn golden_files_son_json_valido_y_tienen_estructura() {
    let dir = golden_dir();
    if !dir.exists() {
        // Los goldens aún no existen, se crearán al ejecutar los tests anteriores
        return;
    }

    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        let contenido = std::fs::read_to_string(&path).unwrap();
        let valor: serde_json::Value = serde_json::from_str(&contenido)
            .unwrap_or_else(|e| panic!("JSON inválido en {}: {e}", path.display()));

        // Verificar estructura mínima del Document
        assert!(
            valor.get("paginas").is_some(),
            "Golden {} debe tener campo 'paginas'",
            path.display()
        );
        assert!(
            valor.get("metricas").is_some(),
            "Golden {} debe tener campo 'metricas'",
            path.display()
        );
        assert!(
            valor.get("ruta_origen").is_some(),
            "Golden {} debe tener campo 'ruta_origen'",
            path.display()
        );
    }
}
