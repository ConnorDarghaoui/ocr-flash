# Changelog

Todos los cambios notables se documentan en este archivo.
Formato basado en [Keep a Changelog](https://keepachangelog.com/es/1.0.0/).

## [Unreleased] — v2.0

### Added — Fase 6: Procesamiento por Lotes y Aceleración GPU
- Subcomando `batch` — procesa directorios completos en paralelo con `rayon::par_iter()`; imprime resumen con completados/fallidos, páginas y tiempo total
- Subcomando `update-checksums` — calcula SHA256 de modelos instalados y actualiza `model_manifest.toml`
- `use_gpu: bool` en `GeneralConfig` y `config/default.toml` (`use_gpu = false` por defecto)
- `ort_session::construir_sesion(path, use_gpu)` — helper compartido con fallback automático CUDA → DirectML → CoreML → CPU
- Los 5 adapters ONNX (layout, OCR det/rec, orientación página/línea, tabla) exponen `new_with_gpu(path, use_gpu)`
- `compose.rs` propaga `config.general.use_gpu` a todos los adapters al construir el sistema
- `ModelManifest::calcular_sha256(path)` — método público para calcular SHA256 de archivos en disco

## [Unreleased]

### Added
- Fase 5: Portabilidad de paths multiplataforma (`AppPaths::resolver()`)
- Modelos y config se almacenan en dirs del usuario (`~/.local/share/reconstructor/`, `~/.config/reconstructor/`)
- Lookup chain para `model_manifest.toml`: exe_dir → data_dir → CWD
- `ModelManifest::cargar_con_base_dir()` para resolver paths relativos del manifest
- GitHub Actions release pipeline (`.github/workflows/release.yml`)
- Script de empaquetado AppImage (`scripts/package-appimage.sh`)
- `README.md` y `CHANGELOG.md`

## [0.4.0] — 2026-03-21

### Added — Fase 4: Evaluación y Optimización
- `EvalReport`, `cer()`, `iou()`, `layout_iou()`, `fallback_rate()`, `throughput_paginas_por_segundo()` en `reconstructor-domain`
- Subcomando CLI `evaluate` — procesa directorio, genera reporte HTML con métricas
- Benchmarks Criterion en `reconstructor-infra` (orientación, layout, OCR, composición)
- `num_threads` y `max_concurrent_pages` en `GeneralConfig`; `inicializar_thread_pool()` en `reconstructor-app`
- 6 snapshot tests de regresión con golden JSON fixtures
- `generar_informe_html()` — reporte HTML auto-contenido con CSS inline

## [0.3.0] — 2026-03-21

### Added — Fase 3: Interfaz de Usuario (Slint)
- Binario `reconstructor-gui` con UI de escritorio completa (6 pantallas)
- Pantalla de verificación/descarga de modelos con barras de progreso y throttling 100ms
- File picker nativo (`rfd`) y drag & drop
- Panel de procesamiento con progreso por página y estado del DocumentFSM
- Preview de thumbnails de páginas reconstruidas
- Panel de métricas (`ProcessingMetrics`) al completar
- Panel de configuración persistido en `config/user.toml`
- Dark mode toggle
- `AppState` con `Arc<Mutex<>>` y comunicación cross-thread via `invoke_from_event_loop`

## [0.2.0] — 2026-03-21

### Added — Fase 2: Composición y Salidas
- `PrintPdfComposer` — genera PDF con texto posicionado, tablas e imágenes raster
- `PdfOutputGenerator`, `TxtOutputGenerator`, `JsonOutputGenerator`
- `PipelineOrchestrator` — orquesta los 3 FSMs con rayon para paralelismo
- `PipelineEvent` — canal Observer hacia la UI
- CLI (`clap`): subcomandos `process`, `download-models`, `check-models`
- Integration tests completos: imagen/PDF → PDF+TXT+JSON

## [0.1.0] — 2026-03-21

### Added — Fases 0 y 1: Dominio e Infraestructura
- Workspace de 4 crates con Onion Architecture
- `reconstructor-domain`: entidades, 3 FSMs (Document, Page, Block), traits (ports), domain services
- `reconstructor-infra`: adapters ONNX (orientación, layout, OCR, tablas), `PdfiumPageReader`, `HuggingFaceModelProvider`
- `model_manifest.toml` con 7 modelos ONNX de HuggingFace y verificación SHA256
- `config/default.toml` con todos los parámetros configurables
