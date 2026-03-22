// =============================================================================
// reconstructor-ui — Composition Root + CLI entry point
//
// Propósito: Punto de entrada del binario `reconstructor`. Parsea argumentos CLI,
//            carga configuración y despacha a los subcomandos.
//
// Subcomandos:
//   process           — Ejecuta el pipeline OCR sobre un archivo de entrada
//   batch             — Procesa un directorio completo en paralelo
//   download-models   — Descarga modelos ONNX desde HuggingFace
//   check-models      — Verifica presencia e integridad de los modelos
//   update-checksums  — Calcula SHA256 de modelos instalados y actualiza el manifest
// =============================================================================

mod cli;
mod compose;
mod events;
mod paths;

use anyhow::{Context, Result};
use clap::Parser;
use reconstructor_app::inicializar_thread_pool;
use reconstructor_domain::{generar_informe_html, ModelEntry, ModelProvider, ModelStatus, PipelineConfig};
use reconstructor_infra::{HuggingFaceModelProvider, ImageInputReader, ModelManifest, PdfiumPageReader};

use cli::{Cli, Comando};
use paths::AppPaths;

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Resolver manifest: el flag --manifest sobreescribe; si no se dio (usa el default
    // "model_manifest.toml"), aplicar el lookup chain de AppPaths.
    let manifest_path: String = if cli.manifest != "model_manifest.toml" {
        // El usuario lo especificó explícitamente
        cli.manifest.clone()
    } else {
        AppPaths::resolver()
            .manifest
            .to_string_lossy()
            .to_string()
    };

    // Cargar config antes de inicializar el logger para usar el log_level del config.
    let config = cargar_config(&cli.config)?;

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(&config.general.log_level))
        .with_writer(std::io::stderr)
        .init();

    // F4.4: Configurar thread pool de rayon según config.general.num_threads
    inicializar_thread_pool(&config.general);

    match cli.comando {
        Comando::Process { input, output_dir, formats, fallback_only } => {
            cmd_process(config, &manifest_path, &input, &output_dir, formats.as_deref(), fallback_only)
        }
        Comando::DownloadModels { only } => cmd_download(&manifest_path, only.as_deref()),
        Comando::CheckModels => cmd_check(&manifest_path),
        Comando::Evaluate { input_dir, output_html, fallback_only, titulo } => {
            cmd_evaluate(config, &manifest_path, &input_dir, &output_html, fallback_only, &titulo)
        }
        Comando::Batch { input_dir, output_dir, workers, formats, fallback_only } => {
            cmd_batch(config, &manifest_path, &input_dir, &output_dir, formats.as_deref(), fallback_only, workers)
        }
        Comando::UpdateChecksums => cmd_update_checksums(&manifest_path),
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn cargar_config(path: &str) -> Result<PipelineConfig> {
    let contenido = std::fs::read_to_string(path)
        .with_context(|| format!("No se pudo leer config: {path}"))?;
    toml::from_str(&contenido).context("Config TOML inválida")
}

/// Resuelve la lista de formatos de salida.
/// Si el CLI especifica `--formats`, tiene prioridad sobre el config.
fn resolver_formats(cli_formats: Option<&str>, config_formats: &[String]) -> Vec<String> {
    cli_formats
        .map(|s| s.split(',').map(|f| f.trim().to_lowercase()).collect())
        .unwrap_or_else(|| config_formats.to_vec())
}

// ─── Subcomandos ──────────────────────────────────────────────────────────────

fn cmd_process(
    config: PipelineConfig,
    manifest_path: &str,
    input: &str,
    output_dir: &str,
    cli_formats: Option<&str>,
    fallback_only: bool,
) -> Result<()> {
    let entries = ModelManifest::cargar(manifest_path)
        .with_context(|| format!("No se pudo cargar manifest: {manifest_path}"))?;

    cmd_process_single(&config, &entries, input, output_dir, cli_formats, fallback_only)?;

    eprintln!("Archivos generados en: {output_dir}");
    Ok(())
}

/// Procesa un único archivo de entrada. Usado por `cmd_process` y `cmd_batch`.
fn cmd_process_single(
    config: &PipelineConfig,
    entries: &[ModelEntry],
    input: &str,
    output_dir: &str,
    cli_formats: Option<&str>,
    fallback_only: bool,
) -> Result<()> {
    let formats = resolver_formats(cli_formats, &config.output.formats);

    let root = compose::construir(config, entries, &formats, fallback_only)
        .context("Error construyendo el sistema")?;

    // Hilo separado para consumir eventos de progreso mientras procesar() bloquea.
    let handle = std::thread::spawn(move || events::imprimir_eventos(root.event_rx));

    // Leer archivo de entrada: PDFs vía pdfium, imágenes vía image crate
    let es_pdf = input.to_lowercase().ends_with(".pdf");
    let paginas = if es_pdf {
        let dpi = config.input.rasterization_dpi.min(600) as u16;
        PdfiumPageReader::new(dpi)
            .leer_archivo(input)
            .with_context(|| format!("No se pudo rasterizar el PDF: {input}"))?
    } else {
        ImageInputReader
            .leer_archivo(input)
            .with_context(|| format!("No se pudo leer el archivo de entrada: {input}"))?
    };

    // Crear directorio de salida si no existe
    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("No se pudo crear el directorio de salida: {output_dir}"))?;

    // Derivar el path base de salida: output_dir/nombre_sin_extension
    let stem = std::path::Path::new(input)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let ruta_salida = format!("{}/{}", output_dir.trim_end_matches('/'), stem);

    root.orchestrator
        .procesar(paginas, &ruta_salida)
        .with_context(|| "Error durante el procesamiento del pipeline")?;

    handle.join().ok();
    Ok(())
}

fn cmd_batch(
    config: PipelineConfig,
    manifest_path: &str,
    input_dir: &str,
    output_dir: &str,
    cli_formats: Option<&str>,
    fallback_only: bool,
    workers: usize,
) -> Result<()> {
    use rayon::prelude::*;
    use std::time::Instant;

    let entries = ModelManifest::cargar(manifest_path)
        .with_context(|| format!("No se pudo cargar manifest: {manifest_path}"))?;

    // Listar archivos de entrada soportados
    let archivos: Vec<std::path::PathBuf> = std::fs::read_dir(input_dir)
        .with_context(|| format!("No se pudo leer el directorio: {input_dir}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            matches!(ext.as_str(), "pdf" | "png" | "jpg" | "jpeg" | "tiff" | "webp")
        })
        .collect();

    if archivos.is_empty() {
        anyhow::bail!("No se encontraron archivos de entrada en {input_dir}");
    }

    let num_workers = if workers == 0 {
        rayon::current_num_threads()
    } else {
        workers
    };

    eprintln!("Procesando {} documentos ({} workers)...", archivos.len(), num_workers);

    // Construir thread pool con el número de workers solicitado
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(num_workers)
        .build()
        .context("No se pudo construir el thread pool")?;

    let inicio_total = Instant::now();

    let resultados: Vec<(String, Result<(u32, f64)>)> = pool.install(|| {
        archivos
            .par_iter()
            .map(|archivo| {
                let nombre = archivo
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("?")
                    .to_string();

                let stem = archivo
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("out")
                    .to_string();
                let doc_output_dir = format!("{}/{}", output_dir.trim_end_matches('/'), stem);

                let inicio = Instant::now();
                let resultado = cmd_process_single(
                    &config,
                    &entries,
                    archivo.to_str().unwrap_or(""),
                    &doc_output_dir,
                    cli_formats,
                    fallback_only,
                );
                let elapsed = inicio.elapsed().as_secs_f64();

                // Contar páginas (best-effort, 1 para imágenes)
                let num_paginas: u32 = if archivo
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_lowercase() == "pdf")
                    .unwrap_or(false)
                {
                    // No re-rasterizamos solo para contar; asumimos 1 para el resumen
                    1
                } else {
                    1
                };

                (nombre, resultado.map(|_| (num_paginas, elapsed)))
            })
            .collect()
    });

    let tiempo_total = inicio_total.elapsed().as_secs_f64();

    // Imprimir resultados
    let mut completados = 0usize;
    let mut fallidos = 0usize;
    let mut total_paginas: u32 = 0;

    for (nombre, resultado) in &resultados {
        match resultado {
            Ok((paginas, elapsed)) => {
                eprintln!("  ✓ {}  ({} págs, {:.1}s)", nombre, paginas, elapsed);
                completados += 1;
                total_paginas += paginas;
            }
            Err(e) => {
                eprintln!("  ✗ {}  ERROR: {}", nombre, e);
                fallidos += 1;
            }
        }
    }

    eprintln!("──────────────────────────────────────────────");
    eprintln!(
        "  Completados: {}/{}  |  Fallidos: {}",
        completados,
        archivos.len(),
        fallidos
    );
    eprintln!("  Total páginas: {}   |  Tiempo: {:.1}s", total_paginas, tiempo_total);

    if fallidos > 0 {
        std::process::exit(1);
    }

    Ok(())
}

fn cmd_download(manifest_path: &str, only: Option<&str>) -> Result<()> {
    let entries = ModelManifest::cargar(manifest_path)
        .with_context(|| format!("No se pudo cargar manifest: {manifest_path}"))?;

    let to_download: Vec<_> = match only {
        Some(id) => {
            let found: Vec<_> = entries.iter().filter(|e| e.id == id).collect();
            if found.is_empty() {
                anyhow::bail!("Modelo '{id}' no encontrado en el manifest");
            }
            found
        }
        None => entries.iter().collect(),
    };

    let provider = HuggingFaceModelProvider;

    for entry in to_download {
        eprintln!("Descargando {} ({} MB)...", entry.nombre, entry.size_mb);

        // Crear directorio del modelo si no existe
        if let Some(parent) = std::path::Path::new(&entry.path_local).parent() {
            std::fs::create_dir_all(parent)?;
        }

        let size_mb = entry.size_mb;
        provider
            .descargar_modelo(
                entry,
                Box::new(move |p| {
                    eprint!(
                        "\r  {:.1} MB / {} MB ({:.0}%)",
                        p.bytes_descargados as f64 / 1_000_000.0,
                        size_mb,
                        p.fraccion() * 100.0,
                    );
                }),
            )
            .with_context(|| format!("Error descargando {}", entry.nombre))?;

        eprintln!("\n  ✓ Guardado en {}", entry.path_local);
    }

    Ok(())
}

fn cmd_check(manifest_path: &str) -> Result<()> {
    let entries = ModelManifest::cargar(manifest_path)
        .with_context(|| format!("No se pudo cargar manifest: {manifest_path}"))?;

    let provider = HuggingFaceModelProvider;
    let statuses = provider.verificar_modelos(&entries);

    let mut all_ok = true;
    println!("{:<30} {:<45} {}", "ID", "Nombre", "Estado");
    println!("{}", "-".repeat(85));

    for (id, status) in &statuses {
        let (icon, label) = match status {
            ModelStatus::Ok => ("✓", "Ok"),
            ModelStatus::Faltante => {
                all_ok = false;
                ("✗", "Faltante")
            }
            ModelStatus::Corrupto => {
                all_ok = false;
                ("✗", "Corrupto")
            }
        };
        let nombre = entries
            .iter()
            .find(|e| &e.id == id)
            .map(|e| e.nombre.as_str())
            .unwrap_or("");
        println!("{icon} {id:<28} {nombre:<45} {label}");
    }

    if !all_ok {
        eprintln!("\nAlgunos modelos faltan. Ejecuta: reconstructor download-models");
        std::process::exit(1);
    }

    Ok(())
}

fn cmd_evaluate(
    config: PipelineConfig,
    manifest_path: &str,
    input_dir: &str,
    output_html: &str,
    fallback_only: bool,
    titulo: &str,
) -> Result<()> {
    use reconstructor_domain::{EvalReport, ProcessingMetrics};
    use std::time::Instant;

    let entries = ModelManifest::cargar(manifest_path)
        .with_context(|| format!("No se pudo cargar manifest: {manifest_path}"))?;

    // Listar archivos de entrada soportados
    let archivos: Vec<std::path::PathBuf> = std::fs::read_dir(input_dir)
        .with_context(|| format!("No se pudo leer el directorio: {input_dir}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            matches!(ext.as_str(), "pdf" | "png" | "jpg" | "jpeg" | "tiff" | "webp")
        })
        .collect();

    if archivos.is_empty() {
        anyhow::bail!("No se encontraron archivos de entrada en {input_dir}");
    }

    eprintln!("Evaluando {} archivo(s) en {}...", archivos.len(), input_dir);

    let formats = vec!["json".to_string()]; // Solo JSON para evaluación
    let temp_dir = std::env::temp_dir().join("reconstructor_evaluate");
    std::fs::create_dir_all(&temp_dir)?;

    let mut reportes: Vec<EvalReport> = Vec::new();
    let mut nombres: Vec<String> = Vec::new();

    for archivo in &archivos {
        let nombre = archivo.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string();
        eprintln!("  Procesando {nombre}...");

        let root = compose::construir(&config, &entries, &formats, fallback_only)
            .context("Error construyendo sistema")?;

        let es_pdf = archivo.extension().and_then(|e| e.to_str())
            .map(|e| e.to_lowercase() == "pdf").unwrap_or(false);

        let paginas = if es_pdf {
            let dpi = config.input.rasterization_dpi.min(600) as u16;
            PdfiumPageReader::new(dpi)
                .leer_archivo(archivo.to_str().unwrap_or(""))
                .with_context(|| format!("Error rasterizando {nombre}"))?
        } else {
            ImageInputReader
                .leer_archivo(archivo.to_str().unwrap_or(""))
                .with_context(|| format!("Error leyendo {nombre}"))?
        };

        let stem = archivo.file_stem().and_then(|s| s.to_str()).unwrap_or("out");
        let ruta = temp_dir.join(stem).to_string_lossy().to_string();

        let inicio = Instant::now();
        let _handle = std::thread::spawn(move || events::imprimir_eventos(root.event_rx));
        root.orchestrator
            .procesar(paginas.clone(), &ruta)
            .with_context(|| format!("Error procesando {nombre}"))?;
        let elapsed_ms = inicio.elapsed().as_secs_f64() * 1000.0;

        // Construir EvalReport con métricas del pipeline
        // (sin ground truth, CER e IoU son 0.0 — se requiere F4.1 para valores reales)
        let metricas = ProcessingMetrics {
            total_paginas: paginas.len() as u32,
            tiempo_total_ms: elapsed_ms,
            tiempo_promedio_por_pagina_ms: elapsed_ms / paginas.len().max(1) as f64,
            ..Default::default()
        };
        let report = EvalReport::new(metricas, 0.0, 0.0);
        reportes.push(report);
        nombres.push(nombre.clone());

        eprintln!("    ✓ {nombre} ({:.1}s)", elapsed_ms / 1000.0);
    }

    // Generar informe HTML
    let html = generar_informe_html(&reportes, titulo, &nombres);
    std::fs::write(output_html, &html)
        .with_context(|| format!("No se pudo escribir el informe en {output_html}"))?;

    let total_pags: u32 = reportes.iter().map(|r| r.pipeline.total_paginas).sum();
    let tp_avg = reportes.iter().map(|r| r.throughput).sum::<f64>() / reportes.len().max(1) as f64;
    let fr_avg = reportes.iter().map(|r| r.fallback_rate).sum::<f64>() / reportes.len().max(1) as f64;

    eprintln!("\n─── Resultados ───────────────────────────────");
    eprintln!("  Documentos: {}", reportes.len());
    eprintln!("  Páginas:    {total_pags}");
    eprintln!("  Throughput: {tp_avg:.2} pág/seg (promedio)");
    eprintln!("  Fallback:   {:.1}% (promedio)", fr_avg * 100.0);
    eprintln!("  Informe:    {output_html}");
    eprintln!("──────────────────────────────────────────────");
    eprintln!("  Nota: CER e IoU requieren ground truth (F4.1).");

    Ok(())
}

fn cmd_update_checksums(manifest_path: &str) -> Result<()> {
    let mut contenido = std::fs::read_to_string(manifest_path)
        .with_context(|| format!("No se pudo leer manifest: {manifest_path}"))?;

    let entries = ModelManifest::cargar(manifest_path)
        .with_context(|| format!("No se pudo cargar manifest: {manifest_path}"))?;

    let mut actualizados = 0usize;
    let mut omitidos = 0usize;

    for entry in &entries {
        let path = std::path::Path::new(&entry.path_local);
        if !path.exists() {
            eprintln!("  - {} (modelo no descargado, omitido)", entry.id);
            omitidos += 1;
            continue;
        }

        let hash = reconstructor_infra::ModelManifest::calcular_sha256(path)
            .with_context(|| format!("Error calculando SHA256 de {}", entry.path_local))?;

        // Reemplazar la línea sha256 = "..." dentro de la sección del modelo.
        // Usamos el id del modelo para localizar la sección y luego reemplazar su sha256.
        let old_sha = format!("sha256 = \"{}\"", entry.sha256);
        let new_sha = format!("sha256 = \"{hash}\"");

        if entry.sha256 == hash {
            eprintln!("  = {} (sin cambios)", entry.id);
            continue;
        }

        if contenido.contains(&old_sha) {
            // Solo reemplazar la primera ocurrencia después de encontrar el id del modelo
            // Para seguridad, reemplazamos de forma global (las secciones son únicas por id)
            contenido = contenido.replacen(&old_sha, &new_sha, 1);
            eprintln!("  ✓ {} → {}", entry.id, &hash[..16]);
            actualizados += 1;
        } else {
            eprintln!("  ? {} (no se pudo localizar sha256 en el manifest)", entry.id);
        }
    }

    std::fs::write(manifest_path, &contenido)
        .with_context(|| format!("No se pudo escribir manifest: {manifest_path}"))?;

    eprintln!("──────────────────────────────────────");
    eprintln!("  Actualizados: {}  |  Omitidos: {}", actualizados, omitidos);

    Ok(())
}
