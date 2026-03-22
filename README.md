# ReconstructOR

Sistema OCR multimodelo de escritorio para reconstrucción fiel de documentos escaneados. Produce PDF, TXT y JSON preservando el layout espacial del original.

## Características

- **Pipeline multimodelo**: orientación de página, detección de layout (DocLayout-YOLO), OCR (PaddleOCR v5), reconocimiento de tablas (SLANet+)
- **Salidas**: PDF reconstruido con texto posicionado, TXT plano y JSON estructurado
- **UI de escritorio**: interfaz Slint con descarga automática de modelos, progreso en tiempo real y preview de páginas
- **Rendimiento**: pipeline nativo en Rust con inferencia ONNX (≥2 páginas/seg en CPU i7)
- **Local**: sin APIs externas, modelos ONNX on-device

## Instalación

### Opción 1 — Descarga precompilada (Linux x86_64)

```bash
# Descargar el release más reciente
tar xzf reconstructor-vX.Y.Z-linux-x86_64.tar.gz
cd reconstructor-vX.Y.Z-linux-x86_64/

# Interfaz gráfica
./reconstructor-gui

# O CLI
./reconstructor --help
```

Los modelos ONNX (~190 MB) se descargan automáticamente en el primer arranque a:
- `~/.local/share/reconstructor/models/` (Linux)
- `%APPDATA%\reconstructor\models\` (Windows)

### Opción 2 — Compilar desde fuente

```bash
git clone <repo-url>
cd reconstructor
cargo build --release --bin reconstructor-gui --bin reconstructor
```

Requisitos: Rust 1.75+, OpenSSL headers, libfontconfig.

## Uso

### Interfaz gráfica

```bash
./reconstructor-gui
```

1. Al primer arranque descarga los modelos automáticamente
2. Selecciona o arrastra el archivo de entrada (PDF, PNG, JPEG, TIFF, WEBP)
3. Pulsa "Procesar"
4. Abre la carpeta de salida para ver los archivos generados

### CLI

```bash
# Procesar un documento
./reconstructor process documento.pdf ./salida/

# Solo texto plano y JSON (sin PDF)
./reconstructor process escaneo.jpg ./salida/ --formats txt,json

# Sin modelos ONNX (solo fallback raster)
./reconstructor process foto.png ./salida/ --fallback-only

# Verificar modelos instalados
./reconstructor check-models

# Descargar modelos
./reconstructor download-models

# Evaluar un directorio (genera informe HTML)
./reconstructor evaluate ./dataset/ --output-html informe.html
```

## Configuración

La configuración se almacena en `~/.config/reconstructor/user.toml` (Linux). Valores por defecto en `config/default.toml` junto al binario.

Parámetros principales:

```toml
[ocr]
confidence_threshold = 0.60   # threshold de confianza OCR (0.0–1.0)

[input]
rasterization_dpi = 300       # DPI para rasterizar PDFs

[output]
formats = ["pdf", "txt", "json"]
```

## Métricas de calidad (targets SRS §10.3)

| Métrica | Target |
|---|---|
| CER (Character Error Rate) | < 5% |
| Layout IoU | > 0.85 |
| Fallback Rate | < 15% |
| Throughput | ≥ 2 páginas/seg |

## Licencia

[A definir — MIT o Apache 2.0]

Slint UI: GPL v3 (compatible con distribución open source).
Modelos: Apache 2.0 (PaddleOCR, DocLayout-YOLO).
