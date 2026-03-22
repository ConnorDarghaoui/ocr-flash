// =============================================================================
// processor — Bridge entre PipelineOrchestrator y la UI (F3.3 + F3.4)
//
// Thread model:
//   worker thread  → compose::construir() + orchestrator.procesar() [bloqueante]
//   event consumer → loop { rx.recv() → invoke_from_event_loop(actualizar UI) }
//
// Thumbnails: al recibir PaginaEstadoCambiada { Done }, convierte la imagen
// PNG de entrada a slint::Image vía SharedPixelBuffer.
// =============================================================================

use std::sync::Arc;

use slint::VecModel;

use crate::{AppWindow, MetricsData, PageThumbnail};

/// Lanza el procesamiento en un hilo worker.
/// Requiere que `selected_file` y `output_dir` estén configurados.
pub fn iniciar_procesamiento(
    window: slint::Weak<AppWindow>,
    selected_file: String,
    output_dir: String,
    config: super::config_store::UserConfig,
    manifest_path: String,
) {
    std::thread::spawn(move || {
        use reconstructor_infra::{ImageInputReader, ModelManifest, PdfiumPageReader};
        use reconstructor_app::events::PipelineEvent;
        use reconstructor_domain::PageState;

        // Navegar a Processing
        {
            let w = window.clone();
            slint::invoke_from_event_loop(move || {
                if let Some(win) = w.upgrade() {
                    win.set_current_page(3);
                    win.set_processing_progress(0.0);
                    win.set_document_state("Iniciando...".into());
                    win.set_current_page_label("".into());
                }
            })
            .ok();
        }

        // Cargar manifest
        let entries = match ModelManifest::cargar(&manifest_path) {
            Ok(e) => e,
            Err(err) => {
                mostrar_error(&window, format!("Error cargando manifest: {err}"));
                return;
            }
        };

        // Construir sistema
        let formats: Vec<String> = config.pipeline.output.formats.clone();
        let root = match crate::compose::construir(
            &config.pipeline,
            &entries,
            &formats,
            false,
        ) {
            Ok(r) => r,
            Err(err) => {
                mostrar_error(&window, format!("Error construyendo sistema: {err}"));
                return;
            }
        };

        // Leer páginas de entrada
        let es_pdf = selected_file.to_lowercase().ends_with(".pdf");
        let dpi = config.pipeline.input.rasterization_dpi.min(600) as u16;

        let paginas = if es_pdf {
            match PdfiumPageReader::new(dpi).leer_archivo(&selected_file) {
                Ok(p) => p,
                Err(err) => {
                    mostrar_error(&window, format!("Error rasterizando PDF: {err}"));
                    return;
                }
            }
        } else {
            match ImageInputReader.leer_archivo(&selected_file) {
                Ok(p) => p,
                Err(err) => {
                    mostrar_error(&window, format!("Error leyendo imagen: {err}"));
                    return;
                }
            }
        };

        let total_paginas = paginas.len();

        // Guardar imágenes para thumbnails (Arc para compartir con consumer thread)
        let paginas_arc = Arc::new(paginas.clone());

        // Actualizar UI con total de páginas
        {
            let w = window.clone();
            slint::invoke_from_event_loop(move || {
                if let Some(win) = w.upgrade() {
                    win.set_document_state(format!("Procesando {total_paginas} página(s)...").into());
                    let thumbs = std::rc::Rc::new(VecModel::<PageThumbnail>::default());
                    win.set_thumbnails(thumbs.into());
                }
            })
            .ok();
        }

        // Preparar ruta de salida
        let stem = std::path::Path::new(&selected_file)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("output");
        let ruta_salida = format!(
            "{}/{}",
            output_dir.trim_end_matches('/'),
            stem
        );

        // Crear directorio de salida
        if let Err(err) = std::fs::create_dir_all(&output_dir) {
            mostrar_error(&window, format!("Error creando directorio de salida: {err}"));
            return;
        }

        // Hilo consumer de eventos
        let event_rx = root.event_rx;
        let w_events = window.clone();
        let paginas_for_events = paginas_arc.clone();
        let total = total_paginas;
        let ruta_salida_worker = ruta_salida.clone();
        let consumer = std::thread::spawn(move || {
            let mut paginas_completas = 0usize;
            loop {
                match event_rx.recv() {
                    Ok(event) => match event {
                        PipelineEvent::DocumentoEstadoCambiado(estado) => {
                            let label: slint::SharedString = format!("{estado:?}").into();
                            let w = w_events.clone();
                            slint::invoke_from_event_loop(move || {
                                if let Some(win) = w.upgrade() {
                                    win.set_document_state(label);
                                }
                            })
                            .ok();
                        }
                        PipelineEvent::PaginaEstadoCambiada { num_pagina, estado } => {
                            if matches!(estado, PageState::Done) {
                                paginas_completas += 1;
                                let progress =
                                    paginas_completas as f32 / total as f32;
                                let label: slint::SharedString =
                                    format!("Página {num_pagina} / {total}").into();

                                // Generar datos crudos de thumbnail (Send)
                                let thumb_raw =
                                    generar_thumbnail_raw(&paginas_for_events, num_pagina);

                                let w = w_events.clone();
                                slint::invoke_from_event_loop(move || {
                                    if let Some(win) = w.upgrade() {
                                        win.set_processing_progress(progress);
                                        win.set_current_page_label(label);
                                        // Convertir a slint::Image aquí (main thread)
                                        if let Some(raw) = thumb_raw {
                                            let thumb = raw_to_thumbnail(raw);
                                            let thumbs = win.get_thumbnails();
                                            use slint::Model;
                                            let mut rows: Vec<PageThumbnail> = (0..thumbs.row_count())
                                                .filter_map(|i| thumbs.row_data(i))
                                                .collect();
                                            rows.push(thumb);
                                            win.set_thumbnails(
                                                std::rc::Rc::new(VecModel::from(rows)).into(),
                                            );
                                        }
                                    }
                                })
                                .ok();
                            }
                        }
                        PipelineEvent::PaginaProgreso { .. } => {}
                        PipelineEvent::BloqueEstadoCambiado { .. } => {}
                        PipelineEvent::ModelosProgreso { .. } => {}
                        PipelineEvent::ProcesamientoCompleto { metricas } => {
                            let w = w_events.clone();
                            let output_path_str = ruta_salida.clone();
                            slint::invoke_from_event_loop(move || {
                                if let Some(win) = w.upgrade() {
                                    win.set_metrics(MetricsData {
                                        total_pages: metricas.total_paginas as i32,
                                        total_blocks: metricas.total_bloques_detectados as i32,
                                        text_blocks: metricas.bloques_resueltos_texto as i32,
                                        table_blocks: metricas.bloques_resueltos_tabla as i32,
                                        raster_blocks: metricas.bloques_fallback_raster as i32,
                                        unresolvable: metricas.bloques_irresolubles as i32,
                                        total_time_ms: metricas.tiempo_total_ms as f32,
                                        avg_time_per_page_ms: metricas
                                            .tiempo_promedio_por_pagina_ms
                                            as f32,
                                    });
                                    win.set_output_path(output_path_str.into());
                                    win.set_current_page(4); // Results
                                }
                            })
                            .ok();
                            break;
                        }
                        PipelineEvent::ErrorGlobal(msg) => {
                            let w = w_events.clone();
                            slint::invoke_from_event_loop(move || {
                                if let Some(win) = w.upgrade() {
                                    win.set_error_message(msg.into());
                                    win.set_current_page(2); // Volver a FileSelect
                                }
                            })
                            .ok();
                            break;
                        }
                    },
                    Err(_) => break, // canal cerrado
                }
            }
        });

        // Procesamiento bloqueante (ruta_salida ya fue movida al consumer thread)
        if let Err(err) = root.orchestrator.procesar(paginas, &ruta_salida_worker) {
            mostrar_error(&window, format!("Error en pipeline: {err}"));
        }

        consumer.join().ok();
    });
}

/// Datos crudos de thumbnail — Send, para pasar entre threads.
struct ThumbnailRaw {
    page_number: usize,
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

/// Genera thumbnail como datos RGBA crudos (sin crear slint::Image, que no es Send).
fn generar_thumbnail_raw(
    paginas: &[reconstructor_domain::PageImage],
    num_pagina: usize,
) -> Option<ThumbnailRaw> {
    let pagina = paginas.iter().find(|p| p.numero_pagina as usize == num_pagina)?;

    let img = image::load_from_memory(&pagina.datos).ok()?;
    let thumb_w = 200u32;
    let thumb_h = ((pagina.alto as f32 / pagina.ancho.max(1) as f32) * thumb_w as f32) as u32;
    let thumb = img.resize(thumb_w, thumb_h.max(1), image::imageops::FilterType::Triangle);
    let rgba = thumb.to_rgba8();

    Some(ThumbnailRaw {
        page_number: num_pagina,
        pixels: rgba.into_raw(),
        width: thumb_w,
        height: thumb_h.max(1),
    })
}

/// Convierte ThumbnailRaw a PageThumbnail con slint::Image.
/// Debe llamarse dentro del event loop (main thread).
fn raw_to_thumbnail(raw: ThumbnailRaw) -> PageThumbnail {
    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        &raw.pixels,
        raw.width,
        raw.height,
    );
    PageThumbnail {
        page_number: raw.page_number as i32,
        image: slint::Image::from_rgba8(buffer),
        completed: true,
    }
}

fn mostrar_error(window: &slint::Weak<AppWindow>, msg: String) {
    let w = window.clone();
    slint::invoke_from_event_loop(move || {
        if let Some(win) = w.upgrade() {
            win.set_error_message(msg.into());
            win.set_current_page(2); // Volver a FileSelect con el error visible
        }
    })
    .ok();
}
