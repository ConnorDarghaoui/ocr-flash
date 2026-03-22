// =============================================================================
// Tests de integración del binario CLI `reconstructor`
//
// Usa CARGO_BIN_EXE_reconstructor (inyectado por Cargo en integration tests)
// para obtener la ruta del binario compilado.
// =============================================================================

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_reconstructor");

// ─── Helpers de fixtures ─────────────────────────────────────────────────────

fn temp_dir_cli(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("reconstructor_cli_test_{}", name));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Escribe un config.toml mínimo en dir y retorna su path.
fn escribir_config_minimo(dir: &Path) -> PathBuf {
    let config = r#"
[general]
models_dir = "models"
log_level = "error"

[input]
rasterization_dpi = 72
supported_formats = ["png"]

[orientation]
page_threshold = 0.85
textline_threshold = 0.90
textline_batch_size = 32

[layout]
detection_threshold = 0.50
nms_threshold = 0.45
input_size = 1024

[ocr]
confidence_threshold = 0.60
max_retries = 0
max_unrecognizable_ratio = 0.30

[table]
structure_threshold = 0.50

[output]
formats = ["pdf", "txt", "json"]
default_font = "Liberation Sans"
default_font_size = 11.0
"#;
    let path = dir.join("config.toml");
    std::fs::write(&path, config).unwrap();
    path
}

/// Escribe un model_manifest.toml vacío (sin modelos) en dir.
fn escribir_manifest_vacio(dir: &Path) -> PathBuf {
    let path = dir.join("manifest.toml");
    std::fs::write(&path, "[models]\n").unwrap();
    path
}

/// Escribe un manifest con una entrada de modelo que apunta a ruta inexistente.
fn escribir_manifest_con_modelo_faltante(dir: &Path) -> PathBuf {
    let contenido = r#"
[models.fake_model]
name = "Fake Model"
repo = "fake/repo"
path = "model.onnx"
local_path = "/tmp/reconstructor_test_nonexistent_model_xyz_never_exists.onnx"
sha256 = ""
size_mb = 1
"#;
    let path = dir.join("manifest_faltante.toml");
    std::fs::write(&path, contenido).unwrap();
    path
}

/// Escribe un PNG sintético 100×100 en la ruta dada.
fn crear_png_sintetico(path: &Path) {
    let img = image::DynamicImage::ImageRgb8(image::RgbImage::new(100, 100));
    let mut bytes = Vec::new();
    img.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
    std::fs::write(path, bytes).unwrap();
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn cli_help_exit_0() {
    let salida = Command::new(BIN).arg("--help").output().unwrap();

    assert!(salida.status.success(), "--help debe salir con código 0");
    let stdout = String::from_utf8_lossy(&salida.stdout);
    assert!(stdout.contains("process"), "help debe mencionar el subcomando 'process'");
}

#[test]
fn cli_check_models_reporta_faltantes() {
    let dir = temp_dir_cli("check_models");
    let config_path = escribir_config_minimo(&dir);
    let manifest_path = escribir_manifest_con_modelo_faltante(&dir);

    let salida = Command::new(BIN)
        .arg("--config").arg(&config_path)
        .arg("--manifest").arg(&manifest_path)
        .arg("check-models")
        .output()
        .unwrap();

    assert!(
        !salida.status.success(),
        "check-models debe fallar cuando hay modelos faltantes"
    );
    let stdout = String::from_utf8_lossy(&salida.stdout);
    assert!(stdout.contains("Faltante"), "stdout debe contener 'Faltante'");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn cli_process_fallback_only_genera_archivos() {
    let dir = temp_dir_cli("process_fallback");
    let config_path = escribir_config_minimo(&dir);
    let manifest_path = escribir_manifest_vacio(&dir);
    let input_path = dir.join("test_input.png");
    let output_dir = dir.join("out");

    crear_png_sintetico(&input_path);

    let salida = Command::new(BIN)
        .arg("--config").arg(&config_path)
        .arg("--manifest").arg(&manifest_path)
        .arg("process")
        .arg("--fallback-only")
        .arg(&input_path)
        .arg(&output_dir)
        .output()
        .unwrap();

    assert!(
        salida.status.success(),
        "process debe salir con código 0. stderr: {}",
        String::from_utf8_lossy(&salida.stderr)
    );
    assert!(output_dir.join("test_input.pdf").exists(), "debe generarse test_input.pdf");
    assert!(output_dir.join("test_input.json").exists(), "debe generarse test_input.json");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn cli_process_input_inexistente_retorna_error() {
    let dir = temp_dir_cli("process_inexistente");
    let config_path = escribir_config_minimo(&dir);
    let manifest_path = escribir_manifest_vacio(&dir);

    let salida = Command::new(BIN)
        .arg("--config").arg(&config_path)
        .arg("--manifest").arg(&manifest_path)
        .arg("process")
        .arg("--fallback-only")
        .arg("/tmp/no_existe_reconstructor_1234567890.png")
        .arg("/tmp/out_reconstructor_test_inexistente")
        .output()
        .unwrap();

    assert!(
        !salida.status.success(),
        "process con input inexistente debe retornar código de error"
    );

    std::fs::remove_dir_all(&dir).ok();
}
