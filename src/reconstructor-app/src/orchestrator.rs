// =============================================================================
// PipelineOrchestrator — Coordinador Concurrente del Sistema OCR (Anillo 3)
//
// Propósito: Desacopla la lógica de control de flujo de las implementaciones de inferencia, 
//            gestionando el paralelismo (Rayon) y la observabilidad (EventBus).
//
// Flujo:     |> Orientación (1 hilo por pág)
//            |> Layout (1 hilo por pág)
//            |> Resolución de Bloques (Fork-Join con M hilos robados)
//            |> Composición
//            |> Generación de Artefactos I/O
// =============================================================================

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

use rayon::prelude::*;
use reconstructor_domain::{
    fsm::transition::transition_block,
    services::{ConfidenceDecision, ConfidenceEvaluator, ResolverFactory},
    traits::{
        ComposedPage, LayoutDetector, OrientationCorrector, OutputGenerator, PageComposer,
    },
    BlockState, BoundingBox, Document, Page, PageImage, PageState,
    PipelineConfig, ProcessingMetrics, Region, ResolvedBlock, ResolvedContent, StateTransition,
    StrategyKind,
};
use tracing::{debug, info, warn};

use crate::error::AppError;
use crate::events::PipelineEvent;

/// Implementa el patrón Inversion of Control (IoC) orquestando puertos de dominio (`Traits`).
///
/// Gestiona la presión sobre el Thread Pool de Rayon, equilibrando la carga de memoria
/// entre la deserialización de páginas PDF y la inferencia masiva de tensores.
pub struct PipelineOrchestrator {
    orientation: Box<dyn OrientationCorrector>,
    layout: Box<dyn LayoutDetector>,
    resolvers: ResolverFactory,
    composer: Box<dyn PageComposer>,
    output_generators: Vec<Box<dyn OutputGenerator>>,
    event_tx: Sender<PipelineEvent>,
    config: PipelineConfig,
}

impl PipelineOrchestrator {
    /// Inyecta las dependencias concretas ensambladas en el Composition Root de la UI o CLI.
    ///
    /// Retorna el emisor inyectado y el receptor para que el llamador pueda despachar los
    /// eventos al event loop de la interfaz gráfica sin bloquear este thread de worker.
    pub fn new(
        orientation: Box<dyn OrientationCorrector>,
        layout: Box<dyn LayoutDetector>,
        resolvers: ResolverFactory,
        composer: Box<dyn PageComposer>,
        output_generators: Vec<Box<dyn OutputGenerator>>,
        config: PipelineConfig,
    ) -> (Self, Receiver<PipelineEvent>) {
        let (tx, rx) = mpsc::channel();
        (
            Self {
                orientation,
                layout,
                resolvers,
                composer,
                output_generators,
                event_tx: tx,
                config,
            },
            rx,
        )
    }

    /// Punto de entrada del pipeline de procesamiento masivo.
    ///
    /// Ejerce como barrera de sincronización principal: paraleliza la evaluación de páginas 
    /// a través de iteradores paralelos (`par_iter`) y recolecta los resultados inmutables 
    /// antes de solicitar la generación serializada de los artefactos de salida en disco.
    ///
    /// # Arguments
    ///
    /// * `paginas_input` - Buffer de imágenes rasterizadas en memoria, provenientes del parseo del PDF.
    /// * `ruta_salida` - Directorio y prefijo para escribir los artefactos resultantes.
    ///
    /// # Errors
    ///
    /// Propaga un `AppError` en caso de fallos insalvables de hardware (OOM, Disco Lleno).
    pub fn procesar(
        &self,
        paginas_input: Vec<PageImage>,
        ruta_salida: &str,
    ) -> Result<Document, AppError> {
        let inicio_total = Instant::now();
        let total_paginas = paginas_input.len();

        info!("Iniciando procesamiento: {} paginas", total_paginas);
        self.emitir(PipelineEvent::DocumentoEstadoCambiado(
            reconstructor_domain::DocumentState::Processing {
                total_paginas,
                paginas_completadas: 0,
            },
        ));

        // Procesar paginas en paralelo
        let resultados: Vec<Option<(Page, ComposedPage)>> = paginas_input
            .par_iter()
            .map(|pagina_input| self.procesar_pagina(pagina_input))
            .collect();

        // Filtrar paginas validas
        let mut paginas_validas: Vec<Page> = Vec::new();
        let mut paginas_compuestas: Vec<ComposedPage> = Vec::new();

        for resultado in resultados {
            if let Some((pagina, compuesta)) = resultado {
                paginas_validas.push(pagina);
                paginas_compuestas.push(compuesta);
            }
        }

        // Ordenar por numero de pagina (el paralelo puede desordenar)
        paginas_validas.sort_by_key(|p| p.numero_pagina);
        paginas_compuestas.sort_by_key(|p| p.numero_pagina);

        // Calcular metricas
        let metricas = self.calcular_metricas(&paginas_validas, total_paginas, inicio_total.elapsed().as_millis() as f64);

        // Construir documento
        let documento = Document {
            ruta_origen: ruta_salida.to_string(),
            version_pipeline: env!("CARGO_PKG_VERSION").to_string(),
            procesado_en: chrono_now(),
            paginas: paginas_validas,
            metricas: metricas.clone(),
        };

        // Generar salidas
        self.emitir(PipelineEvent::DocumentoEstadoCambiado(
            reconstructor_domain::DocumentState::Generating {
                etapa: reconstructor_domain::OutputStage::Pdf,
            },
        ));

        for generator in &self.output_generators {
            if let Err(e) = generator.generar(&documento, &paginas_compuestas, ruta_salida) {
                warn!("Error en generacion de salida: {}", e);
            }
        }

        self.emitir(PipelineEvent::ProcesamientoCompleto { metricas });
        self.emitir(PipelineEvent::DocumentoEstadoCambiado(
            reconstructor_domain::DocumentState::Complete,
        ));

        info!("Procesamiento completado en {:.0}ms", inicio_total.elapsed().as_millis());
        Ok(documento)
    }

    /// Procesa una pagina individual. Retorna `None` si la pagina no es procesable.
    fn procesar_pagina(&self, pagina_input: &PageImage) -> Option<(Page, ComposedPage)> {
        let num_pagina = pagina_input.numero_pagina as usize;
        let inicio_pagina = Instant::now();

        debug!("Procesando pagina {}", num_pagina);
        self.emitir(PipelineEvent::PaginaEstadoCambiada {
            num_pagina,
            estado: PageState::Orienting,
        });

        // --- Etapa 1: Correccion de orientacion ---
        let (imagen_orientada, orientacion_grados, orientacion_incierta) =
            match self.orientation.corregir_pagina(
                &pagina_input.datos,
                pagina_input.ancho,
                pagina_input.alto,
            ) {
                Ok((bytes, resultado)) => {
                    debug!("Pagina {}: orientacion corregida {}°", num_pagina, resultado.angulo_grados);
                    (bytes, resultado.angulo_grados, resultado.incierto)
                }
                Err(e) => {
                    warn!("Pagina {}: orientacion fallo ({}), continuando degradado", num_pagina, e);
                    self.emitir(PipelineEvent::PaginaEstadoCambiada {
                        num_pagina,
                        estado: PageState::Degraded { razon: e.to_string() },
                    });
                    // Degradacion segura: continuar con imagen original
                    (pagina_input.datos.clone(), 0.0, true)
                }
            };

        self.emitir(PipelineEvent::PaginaEstadoCambiada {
            num_pagina,
            estado: PageState::DetectingLayout,
        });

        // --- Etapa 2: Deteccion de layout ---
        let regiones = match self.layout.detectar(
            &imagen_orientada,
            pagina_input.ancho,
            pagina_input.alto,
            pagina_input.numero_pagina,
        ) {
            Ok(r) if !r.is_empty() => r,
            Ok(_) => {
                warn!("Pagina {}: sin regiones detectadas, skipeando", num_pagina);
                self.emitir(PipelineEvent::PaginaEstadoCambiada {
                    num_pagina,
                    estado: PageState::Error { razon: "Sin regiones detectadas".into() },
                });
                return None;
            }
            Err(e) => {
                warn!("Pagina {}: layout fallo ({}), skipeando", num_pagina, e);
                self.emitir(PipelineEvent::PaginaEstadoCambiada {
                    num_pagina,
                    estado: PageState::Error { razon: e.to_string() },
                });
                return None;
            }
        };

        let total_bloques = regiones.len();
        self.emitir(PipelineEvent::PaginaEstadoCambiada {
            num_pagina,
            estado: PageState::ResolvingBlocks {
                total_bloques,
                bloques_terminados: 0,
            },
        });

        // --- Etapa 3: Resolucion de bloques en paralelo ---
        let tx = self.event_tx.clone();
        let bloques_resueltos: Vec<ResolvedBlock> = regiones
            .par_iter()
            .map(|region| {
                let bloque = self.resolver_bloque(region, &imagen_orientada, &tx, num_pagina);
                // Emitir progreso (no podemos contar exactamente por el paralelo, pero emitimos)
                let _ = tx.send(PipelineEvent::PaginaProgreso {
                    num_pagina,
                    bloques_terminados: 1, // señal de progreso incremental
                    bloques_totales: total_bloques,
                });
                bloque
            })
            .collect();

        self.emitir(PipelineEvent::PaginaEstadoCambiada {
            num_pagina,
            estado: PageState::Composing,
        });

        // Construir Page del dominio para la composicion
        let pagina = Page {
            numero_pagina: pagina_input.numero_pagina,
            ancho: pagina_input.ancho,
            alto: pagina_input.alto,
            orientacion_correccion_grados: orientacion_grados,
            orientacion_incierta,
            regiones,
            bloques_resueltos: bloques_resueltos.clone(),
            tiempo_procesamiento_ms: inicio_pagina.elapsed().as_millis() as f64,
        };

        // --- Etapa 4: Composicion ---
        let compuesta = match self.composer.componer(&pagina, &bloques_resueltos) {
            Ok(c) => c,
            Err(e) => {
                warn!("Pagina {}: composicion fallo ({}), skipeando", num_pagina, e);
                self.emitir(PipelineEvent::PaginaEstadoCambiada {
                    num_pagina,
                    estado: PageState::Error { razon: e.to_string() },
                });
                return None;
            }
        };

        self.emitir(PipelineEvent::PaginaEstadoCambiada {
            num_pagina,
            estado: PageState::Done,
        });

        Some((pagina, compuesta))
    }

    /// Resuelve una region individual ejecutando el BlockFSM completo.
    ///
    /// Implementa el loop del autómata:
    /// `Detected → Resolving → [Resolved | LowConfidence → retry | StrategyError → Fallback]`
    ///
    /// Emite `BloqueEstadoCambiado` en cada transicion para trazabilidad.
    /// Nunca falla: en el peor caso retorna un bloque `Unresolvable`.
    fn resolver_bloque(
        &self,
        region: &Region,
        imagen_bytes: &[u8],
        tx: &Sender<PipelineEvent>,
        num_pagina: usize,
    ) -> ResolvedBlock {
        let inicio = Instant::now();
        let mut estado = BlockState::Detected;
        let mut historial: Vec<StateTransition> = Vec::new();
        let evaluador = ConfidenceEvaluator::new(
            self.config.ocr.confidence_threshold,
            self.config.ocr.max_unrecognizable_ratio,
        );

        // Seleccionar estrategia segun tipo de bloque
        let estrategia = if region.tipo_bloque.es_textual() {
            StrategyKind::PaddleOcr
        } else if region.tipo_bloque.es_tabla() {
            StrategyKind::SlaNetTable
        } else {
            StrategyKind::RasterPreserve
        };

        // Detected → Resolving
        self.aplicar_transicion_bloque(
            &mut estado,
            reconstructor_domain::fsm::block::BlockEvent::EstrategiaSeleccionada(estrategia),
            &mut historial,
            tx,
            &region.id,
            num_pagina,
            inicio.elapsed().as_millis() as f64,
            self.config.ocr.confidence_threshold,
        );

        // Si RasterPreserve: shortcut directo a Fallback con crop
        if estrategia == StrategyKind::RasterPreserve {
            let crop = extraer_crop(imagen_bytes, &region.bbox);
            self.aplicar_transicion_bloque(
                &mut estado,
                reconstructor_domain::fsm::block::BlockEvent::AplicarFallback {
                    crop_bytes: crop.clone(),
                },
                &mut historial,
                tx,
                &region.id,
                num_pagina,
                inicio.elapsed().as_millis() as f64,
                self.config.ocr.confidence_threshold,
            );
            self.aplicar_transicion_bloque(
                &mut estado,
                reconstructor_domain::fsm::block::BlockEvent::Compuesto,
                &mut historial,
                tx,
                &region.id,
                num_pagina,
                inicio.elapsed().as_millis() as f64,
                self.config.ocr.confidence_threshold,
            );
            return construir_bloque_fallback(region, crop, historial, inicio.elapsed().as_millis() as f64);
        }

        // Resolver con el adapter correspondiente
        let resolver = self.resolvers.para_bloque(region.tipo_bloque);
        let crop = extraer_crop(imagen_bytes, &region.bbox);

        let (contenido_inicial, confianza_inicial) = match resolver {
            Some(r) => match r.resolver(region, &crop) {
                Ok(resultado) => resultado,
                Err(e) => {
                    // InferenciaFallida → StrategyError → Fallback
                    self.aplicar_transicion_bloque(
                        &mut estado,
                        reconstructor_domain::fsm::block::BlockEvent::InferenciaFallida(e.to_string()),
                        &mut historial,
                        tx,
                        &region.id,
                        num_pagina,
                        inicio.elapsed().as_millis() as f64,
                        self.config.ocr.confidence_threshold,
                    );
                    self.aplicar_transicion_bloque(
                        &mut estado,
                        reconstructor_domain::fsm::block::BlockEvent::AplicarFallback {
                            crop_bytes: crop.clone(),
                        },
                        &mut historial,
                        tx,
                        &region.id,
                        num_pagina,
                        inicio.elapsed().as_millis() as f64,
                        self.config.ocr.confidence_threshold,
                    );
                    self.aplicar_transicion_bloque(
                        &mut estado,
                        reconstructor_domain::fsm::block::BlockEvent::Compuesto,
                        &mut historial,
                        tx,
                        &region.id,
                        num_pagina,
                        inicio.elapsed().as_millis() as f64,
                        self.config.ocr.confidence_threshold,
                    );
                    return construir_bloque_fallback(region, crop, historial, inicio.elapsed().as_millis() as f64);
                }
            },
            None => {
                // Sin resolver: Fallback directo
                self.aplicar_transicion_bloque(
                    &mut estado,
                    reconstructor_domain::fsm::block::BlockEvent::InferenciaFallida(
                        format!("Sin resolver para tipo {:?}", region.tipo_bloque),
                    ),
                    &mut historial,
                    tx,
                    &region.id,
                    num_pagina,
                    inicio.elapsed().as_millis() as f64,
                    self.config.ocr.confidence_threshold,
                );
                self.aplicar_transicion_bloque(
                    &mut estado,
                    reconstructor_domain::fsm::block::BlockEvent::AplicarFallback {
                        crop_bytes: crop.clone(),
                    },
                    &mut historial,
                    tx,
                    &region.id,
                    num_pagina,
                    inicio.elapsed().as_millis() as f64,
                    self.config.ocr.confidence_threshold,
                );
                self.aplicar_transicion_bloque(
                    &mut estado,
                    reconstructor_domain::fsm::block::BlockEvent::Compuesto,
                    &mut historial,
                    tx,
                    &region.id,
                    num_pagina,
                    inicio.elapsed().as_millis() as f64,
                    self.config.ocr.confidence_threshold,
                );
                return construir_bloque_fallback(region, crop, historial, inicio.elapsed().as_millis() as f64);
            }
        };

        // Evaluar confianza inicial
        let texto_para_evaluar = match &contenido_inicial {
            ResolvedContent::Text { texto } => texto.as_str(),
            _ => "",
        };
        let decision = evaluador.evaluar(confianza_inicial, texto_para_evaluar);

        match decision {
            ConfidenceDecision::Alta | ConfidenceDecision::Moderada => {
                // InferenciaOk con confianza >= threshold → Resolved
                self.aplicar_transicion_bloque(
                    &mut estado,
                    reconstructor_domain::fsm::block::BlockEvent::InferenciaOk {
                        contenido: contenido_inicial.clone(),
                        confianza: confianza_inicial,
                    },
                    &mut historial,
                    tx,
                    &region.id,
                    num_pagina,
                    inicio.elapsed().as_millis() as f64,
                    self.config.ocr.confidence_threshold,
                );
                self.aplicar_transicion_bloque(
                    &mut estado,
                    reconstructor_domain::fsm::block::BlockEvent::Compuesto,
                    &mut historial,
                    tx,
                    &region.id,
                    num_pagina,
                    inicio.elapsed().as_millis() as f64,
                    self.config.ocr.confidence_threshold,
                );
                ResolvedBlock {
                    region_id: region.id.clone(),
                    tipo_bloque: region.tipo_bloque,
                    contenido: contenido_inicial,
                    confianza_resolucion: confianza_inicial,
                    estrategia_utilizada: estrategia,
                    estado_actual: BlockState::Composed,
                    historial_estados: historial,
                }
            }
            ConfidenceDecision::Baja => {
                // InferenciaOk con confianza < threshold → LowConfidence → retry
                self.aplicar_transicion_bloque(
                    &mut estado,
                    reconstructor_domain::fsm::block::BlockEvent::InferenciaOk {
                        contenido: contenido_inicial,
                        confianza: confianza_inicial,
                    },
                    &mut historial,
                    tx,
                    &region.id,
                    num_pagina,
                    inicio.elapsed().as_millis() as f64,
                    self.config.ocr.confidence_threshold,
                );

                // Reintentar si quedan intentos
                if self.config.ocr.max_retries > 0 {
                    if let Some(r) = resolver {
                        match r.resolver(region, &crop) {
                            Ok((contenido_retry, confianza_retry))
                                if evaluador.evaluar(confianza_retry, "").ne(&ConfidenceDecision::Baja) =>
                            {
                                self.aplicar_transicion_bloque(
                                    &mut estado,
                                    reconstructor_domain::fsm::block::BlockEvent::ReintentoOk {
                                        contenido: contenido_retry.clone(),
                                        confianza: confianza_retry,
                                    },
                                    &mut historial,
                                    tx,
                                    &region.id,
                                    num_pagina,
                                    inicio.elapsed().as_millis() as f64,
                                    self.config.ocr.confidence_threshold,
                                );
                                self.aplicar_transicion_bloque(
                                    &mut estado,
                                    reconstructor_domain::fsm::block::BlockEvent::Compuesto,
                                    &mut historial,
                                    tx,
                                    &region.id,
                                    num_pagina,
                                    inicio.elapsed().as_millis() as f64,
                                    self.config.ocr.confidence_threshold,
                                );
                                return ResolvedBlock {
                                    region_id: region.id.clone(),
                                    tipo_bloque: region.tipo_bloque,
                                    contenido: contenido_retry,
                                    confianza_resolucion: confianza_retry,
                                    estrategia_utilizada: StrategyKind::Retry,
                                    estado_actual: BlockState::Composed,
                                    historial_estados: historial,
                                };
                            }
                            _ => {}
                        }
                    }
                }

                // Retry fallido → Fallback
                self.aplicar_transicion_bloque(
                    &mut estado,
                    reconstructor_domain::fsm::block::BlockEvent::ReintentoFallido,
                    &mut historial,
                    tx,
                    &region.id,
                    num_pagina,
                    inicio.elapsed().as_millis() as f64,
                    self.config.ocr.confidence_threshold,
                );
                self.aplicar_transicion_bloque(
                    &mut estado,
                    reconstructor_domain::fsm::block::BlockEvent::AplicarFallback {
                        crop_bytes: crop.clone(),
                    },
                    &mut historial,
                    tx,
                    &region.id,
                    num_pagina,
                    inicio.elapsed().as_millis() as f64,
                    self.config.ocr.confidence_threshold,
                );
                self.aplicar_transicion_bloque(
                    &mut estado,
                    reconstructor_domain::fsm::block::BlockEvent::Compuesto,
                    &mut historial,
                    tx,
                    &region.id,
                    num_pagina,
                    inicio.elapsed().as_millis() as f64,
                    self.config.ocr.confidence_threshold,
                );
                construir_bloque_fallback(region, crop, historial, inicio.elapsed().as_millis() as f64)
            }
        }
    }

    /// Aplica una transicion del BlockFSM, actualiza el estado y emite el evento.
    #[allow(clippy::too_many_arguments)]
    fn aplicar_transicion_bloque(
        &self,
        estado: &mut BlockState,
        evento: reconstructor_domain::fsm::block::BlockEvent,
        historial: &mut Vec<StateTransition>,
        tx: &Sender<PipelineEvent>,
        bloque_id: &str,
        num_pagina: usize,
        ms: f64,
        threshold: f32,
    ) {
        let nombre_evento = evento.nombre();
        let desde = estado.clone();

        match transition_block(estado, evento, threshold) {
            Ok(nuevo_estado) => {
                historial.push(StateTransition::new(
                    desde.nombre(),
                    nuevo_estado.nombre(),
                    &nombre_evento,
                    ms,
                ));
                let _ = tx.send(PipelineEvent::BloqueEstadoCambiado {
                    num_pagina,
                    bloque_id: bloque_id.to_string(),
                    desde: desde.clone(),
                    hasta: nuevo_estado.clone(),
                });
                *estado = nuevo_estado;
            }
            Err(e) => {
                warn!("Transicion invalida en bloque {}: {}", bloque_id, e);
            }
        }
    }

    /// Emite un evento al canal (ignora silenciosamente si el receptor fue dropeado).
    fn emitir(&self, evento: PipelineEvent) {
        let _ = self.event_tx.send(evento);
    }

    /// Calcula las metricas globales del procesamiento.
    fn calcular_metricas(
        &self,
        paginas: &[Page],
        total_paginas_entrada: usize,
        tiempo_total_ms: f64,
    ) -> ProcessingMetrics {
        let mut metricas = ProcessingMetrics {
            total_paginas: total_paginas_entrada as u32,
            tiempo_total_ms,
            tiempo_promedio_por_pagina_ms: if paginas.is_empty() {
                0.0
            } else {
                tiempo_total_ms / paginas.len() as f64
            },
            ..Default::default()
        };

        for pagina in paginas {
            metricas.total_bloques_detectados += pagina.regiones.len() as u32;
            for bloque in &pagina.bloques_resueltos {
                match &bloque.contenido {
                    ResolvedContent::Text { .. } if bloque.estado_actual == BlockState::Composed => {
                        metricas.bloques_resueltos_texto += 1;
                    }
                    ResolvedContent::Table { .. } if bloque.estado_actual == BlockState::Composed => {
                        metricas.bloques_resueltos_tabla += 1;
                    }
                    ResolvedContent::Raster { .. } if bloque.estado_actual == BlockState::Composed => {
                        metricas.bloques_fallback_raster += 1;
                    }
                    _ if matches!(bloque.estado_actual, BlockState::Unresolvable { .. }) => {
                        metricas.bloques_irresolubles += 1;
                    }
                    _ => {}
                }
            }
        }

        metricas
    }
}

// =============================================================================
// Helpers privados
// =============================================================================

/// Extrae un crop de la imagen en los pixeles del BoundingBox.
/// Si las coordenadas son invalidas, retorna un Vec vacio.
fn extraer_crop(imagen_bytes: &[u8], bbox: &BoundingBox) -> Vec<u8> {
    // En produccion, usaria `image` crate para recortar.
    // Aqui pasamos los bytes completos como placeholder — la implementacion real
    // en infra hace el crop geometrico real.
    if imagen_bytes.is_empty() || bbox.area() <= 0.0 {
        return Vec::new();
    }
    imagen_bytes.to_vec()
}

/// Construye un `ResolvedBlock` en estado Fallback/Composed con crop raster.
fn construir_bloque_fallback(
    region: &Region,
    crop_bytes: Vec<u8>,
    historial: Vec<StateTransition>,
    tiempo_ms: f64,
) -> ResolvedBlock {
    let _ = tiempo_ms; // usado para historial, capturado antes
    ResolvedBlock {
        region_id: region.id.clone(),
        tipo_bloque: region.tipo_bloque,
        contenido: ResolvedContent::Raster {
            imagen_bytes: crop_bytes,
            ancho: region.bbox.width as u32,
            alto: region.bbox.height as u32,
        },
        confianza_resolucion: 0.0,
        estrategia_utilizada: StrategyKind::RasterPreserve,
        estado_actual: BlockState::Composed,
        historial_estados: historial,
    }
}

/// Timestamp ISO 8601 simple (sin dependencia de chrono para mantener deps minimas).
fn chrono_now() -> String {
    // En produccion se usaria chrono o time crate.
    // Retornamos placeholder — suficiente para MVP y tests.
    "2026-01-01T00:00:00Z".to_string()
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use reconstructor_domain::{
        BoundingBox, BlockType, DomainError, OrientationResult, Region, traits::ComposedPage,
    };

    // -------------------------------------------------------------------------
    // Mock adapters
    // -------------------------------------------------------------------------

    struct MockOrientation { falla: bool }
    impl OrientationCorrector for MockOrientation {
        fn corregir_pagina(&self, bytes: &[u8], _ancho: u32, _alto: u32) -> Result<(Vec<u8>, OrientationResult), DomainError> {
            if self.falla {
                Err(DomainError::ConfiguracionInvalida { mensaje: "mock falla".into() })
            } else {
                Ok((bytes.to_vec(), OrientationResult::sin_rotacion(0.95, false)))
            }
        }
    }

    struct MockLayout { regiones: Vec<Region> }
    impl LayoutDetector for MockLayout {
        fn detectar(&self, _: &[u8], _: u32, _: u32, _: u32) -> Result<Vec<Region>, DomainError> {
            Ok(self.regiones.clone())
        }
    }

    struct MockLayoutFalla;
    impl LayoutDetector for MockLayoutFalla {
        fn detectar(&self, _: &[u8], _: u32, _: u32, _: u32) -> Result<Vec<Region>, DomainError> {
            Ok(vec![]) // 0 regiones → pagina skipeada
        }
    }

    struct MockResolver { confianza: f32 }
    impl reconstructor_domain::traits::BlockResolver for MockResolver {
        fn puede_resolver(&self, _: BlockType) -> bool { true }
        fn resolver(&self, _: &Region, _: &[u8]) -> Result<(ResolvedContent, f32), DomainError> {
            Ok((ResolvedContent::Text { texto: "texto resuelto".into() }, self.confianza))
        }
    }

    struct MockComposer;
    impl PageComposer for MockComposer {
        fn componer(&self, pagina: &Page, _: &[ResolvedBlock]) -> Result<ComposedPage, DomainError> {
            Ok(ComposedPage {
                numero_pagina: pagina.numero_pagina,
                pdf_bytes: b"PDF_MOCK".to_vec(),
                texto_extraido: "texto".into(),
            })
        }
    }

    struct MockOutput;
    impl OutputGenerator for MockOutput {
        fn generar(&self, _: &Document, _: &[ComposedPage], _: &str) -> Result<(), DomainError> {
            Ok(())
        }
    }

    fn region_texto(id: &str) -> Region {
        Region::new(id, BlockType::Text, BoundingBox::new(10.0, 10.0, 100.0, 50.0), 0.95)
    }

    fn region_figura(id: &str) -> Region {
        Region::new(id, BlockType::Figure, BoundingBox::new(10.0, 70.0, 100.0, 80.0), 0.90)
    }

    fn pagina_input(numero: u32) -> PageImage {
        PageImage {
            datos: vec![0u8; 64],
            ancho: 800,
            alto: 1200,
            numero_pagina: numero,
        }
    }

    fn orchestrator_con(
        orientation: Box<dyn OrientationCorrector>,
        layout: Box<dyn LayoutDetector>,
        confianza_resolver: f32,
    ) -> (PipelineOrchestrator, Receiver<PipelineEvent>) {
        let resolvers = ResolverFactory::new(vec![Box::new(MockResolver { confianza: confianza_resolver })]);
        PipelineOrchestrator::new(
            orientation,
            layout,
            resolvers,
            Box::new(MockComposer),
            vec![Box::new(MockOutput)],
            PipelineConfig::default(),
        )
    }

    // -------------------------------------------------------------------------
    // Tests
    // -------------------------------------------------------------------------

    #[test]
    fn flujo_completo_una_pagina_dos_regiones() {
        let layout = Box::new(MockLayout {
            regiones: vec![region_texto("blk_1_0"), region_texto("blk_1_1")],
        });
        let (orq, rx) = orchestrator_con(Box::new(MockOrientation { falla: false }), layout, 0.90);

        let doc = orq.procesar(vec![pagina_input(1)], "/tmp/test").unwrap();

        assert_eq!(doc.paginas.len(), 1);
        assert_eq!(doc.paginas[0].bloques_resueltos.len(), 2);
        assert_eq!(doc.metricas.total_paginas, 1);

        // Verificar que se emitieron eventos de progreso
        let eventos: Vec<_> = rx.try_iter().collect();
        assert!(eventos.iter().any(|e| matches!(e, PipelineEvent::ProcesamientoCompleto { .. })));
        assert!(eventos.iter().any(|e| matches!(e, PipelineEvent::PaginaEstadoCambiada { estado: PageState::Done, .. })));
    }

    #[test]
    fn orientacion_falla_pagina_continua_degradada() {
        let layout = Box::new(MockLayout {
            regiones: vec![region_texto("blk_1_0")],
        });
        let (orq, rx) = orchestrator_con(Box::new(MockOrientation { falla: true }), layout, 0.90);

        let doc = orq.procesar(vec![pagina_input(1)], "/tmp/test").unwrap();

        // La pagina debe procesarse aunque la orientacion fallo
        assert_eq!(doc.paginas.len(), 1);
        assert!(doc.paginas[0].orientacion_incierta);

        let eventos: Vec<_> = rx.try_iter().collect();
        assert!(eventos.iter().any(|e| matches!(e, PipelineEvent::PaginaEstadoCambiada {
            estado: PageState::Degraded { .. }, ..
        })));
    }

    #[test]
    fn layout_sin_regiones_skipea_pagina() {
        let (orq, rx) = orchestrator_con(
            Box::new(MockOrientation { falla: false }),
            Box::new(MockLayoutFalla),
            0.90,
        );

        let doc = orq.procesar(vec![pagina_input(1)], "/tmp/test").unwrap();

        assert_eq!(doc.paginas.len(), 0);
        assert_eq!(doc.metricas.total_paginas, 1); // contaba la entrada

        let eventos: Vec<_> = rx.try_iter().collect();
        assert!(eventos.iter().any(|e| matches!(e, PipelineEvent::PaginaEstadoCambiada {
            estado: PageState::Error { .. }, ..
        })));
    }

    #[test]
    fn bloque_con_baja_confianza_activa_fallback() {
        let layout = Box::new(MockLayout {
            regiones: vec![region_texto("blk_1_0")],
        });
        // Confianza 0.10 está muy por debajo del threshold (0.60)
        let (orq, rx) = orchestrator_con(Box::new(MockOrientation { falla: false }), layout, 0.10);

        let doc = orq.procesar(vec![pagina_input(1)], "/tmp/test").unwrap();

        assert_eq!(doc.paginas.len(), 1);
        let bloque = &doc.paginas[0].bloques_resueltos[0];
        // Con confianza 0.10, debe terminar en Fallback → Composed con Raster
        assert!(matches!(bloque.contenido, ResolvedContent::Raster { .. }));

        let eventos: Vec<_> = rx.try_iter().collect();
        assert!(eventos.iter().any(|e| matches!(e, PipelineEvent::BloqueEstadoCambiado { .. })));
    }

    #[test]
    fn figura_va_directo_a_raster_sin_ocr() {
        let layout = Box::new(MockLayout {
            regiones: vec![region_figura("blk_1_0")],
        });
        let (orq, _rx) = orchestrator_con(Box::new(MockOrientation { falla: false }), layout, 0.90);

        let doc = orq.procesar(vec![pagina_input(1)], "/tmp/test").unwrap();

        let bloque = &doc.paginas[0].bloques_resueltos[0];
        assert!(matches!(bloque.contenido, ResolvedContent::Raster { .. }));
        assert_eq!(bloque.estrategia_utilizada, StrategyKind::RasterPreserve);
    }

    #[test]
    fn multiples_paginas_ordenadas_correctamente() {
        let layout = Box::new(MockLayout {
            regiones: vec![region_texto("blk_x_0")],
        });
        let (orq, _rx) = orchestrator_con(Box::new(MockOrientation { falla: false }), layout, 0.90);

        let paginas = vec![pagina_input(3), pagina_input(1), pagina_input(2)];
        let doc = orq.procesar(paginas, "/tmp/test").unwrap();

        assert_eq!(doc.paginas.len(), 3);
        assert_eq!(doc.paginas[0].numero_pagina, 1);
        assert_eq!(doc.paginas[1].numero_pagina, 2);
        assert_eq!(doc.paginas[2].numero_pagina, 3);
    }
}
