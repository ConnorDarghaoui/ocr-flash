// =============================================================================
// JobManager — Delegación asíncrona y ciclo de vida (Worker Thread)
//
// Propósito: Desacopla la ejecución pesada del `PipelineOrchestrator` del Event Loop 
//            de la interfaz gráfica para prevenir bloqueos de UI (Application Not Responding).
//            Actúa como un Future rústico gestionando la máquina de estados del Thread.
// =============================================================================

use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use reconstructor_domain::{Document, PageImage};

use crate::error::AppError;
use crate::orchestrator::PipelineOrchestrator;

#[allow(dead_code)]
enum JobEstado {
    Inactivo,
    Procesando(JoinHandle<Result<Document, AppError>>),
    Completado(Document),
    Error(String),
}

/// Gestor de concurrencia gruesa para aislar el Event Loop del framework gráfico.
pub struct JobManager {
    estado: Arc<Mutex<JobEstado>>,
}

impl JobManager {
    pub fn new() -> Self {
        Self {
            estado: Arc::new(Mutex::new(JobEstado::Inactivo)),
        }
    }

    /// Despacha la carga de trabajo computacional a un hilo nativo del sistema operativo.
    ///
    /// Transfiere la propiedad de los datos brutos (`PageImage`) al worker thread, 
    /// liberando la memoria del hilo principal.
    ///
    /// # Arguments
    ///
    /// * `orchestrator` - Referencia atómica (Arc) al coordinador de inferencias.
    /// * `paginas` - Vectores rasterizados desde el documento original.
    /// * `ruta_salida` - Prefijo de sistema de archivos para exportar los artefactos generados.
    pub fn iniciar(
        &self,
        orchestrator: Arc<PipelineOrchestrator>,
        paginas: Vec<PageImage>,
        ruta_salida: String,
    ) {
        let estado = Arc::clone(&self.estado);

        let handle = thread::spawn(move || {
            orchestrator.procesar(paginas, &ruta_salida)
        });

        *estado.lock().unwrap() = JobEstado::Procesando(handle);
    }

    pub fn esta_activo(&self) -> bool {
        let estado = self.estado.lock().unwrap();
        matches!(*estado, JobEstado::Procesando(_))
    }

    /// Actúa como un mecanismo de "polling" no bloqueante para consumir el resultado
    /// final del procesamiento sin utilizar `await` ni runtimes pesados como Tokio.
    ///
    /// # Returns
    ///
    /// Retorna `None` si el Thread nativo sigue ejecutándose o ya fue consumido.
    /// Retorna `Some` transicionando internamente a `Completado` o `Error` al finalizar.
    pub fn verificar_resultado(&self) -> Option<Result<Document, AppError>> {
        let mut estado = self.estado.lock().unwrap();

        let terminado = if let JobEstado::Procesando(handle) = &*estado {
            handle.is_finished()
        } else {
            false
        };

        if !terminado {
            return None;
        }

        let estado_actual = std::mem::replace(&mut *estado, JobEstado::Inactivo);
        if let JobEstado::Procesando(handle) = estado_actual {
            match handle.join() {
                Ok(Ok(doc)) => {
                    *estado = JobEstado::Completado(doc.clone());
                    Some(Ok(doc))
                }
                Ok(Err(e)) => {
                    let msg = e.to_string();
                    *estado = JobEstado::Error(msg.clone());
                    Some(Err(AppError::Pipeline {
                        etapa: "procesamiento".into(),
                        detalle: msg,
                    }))
                }
                Err(_panic) => {
                    let msg = "El thread de procesamiento termino con panic".to_string();
                    *estado = JobEstado::Error(msg.clone());
                    Some(Err(AppError::Pipeline {
                        etapa: "procesamiento".into(),
                        detalle: msg,
                    }))
                }
            }
        } else {
            None
        }
    }

    pub fn resetear(&self) {
        *self.estado.lock().unwrap() = JobEstado::Inactivo;
    }
}

impl Default for JobManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reconstructor_domain::{
        services::ResolverFactory, traits::{ComposedPage, PageComposer},
        BoundingBox, BlockType, Document, DomainError, OrientationResult, Page, PageImage,
        PipelineConfig, ResolvedBlock, ResolvedContent, Region,
        traits::{LayoutDetector, OrientationCorrector, OutputGenerator},
    };
    use crate::orchestrator::PipelineOrchestrator;

    struct MockOk;
    impl OrientationCorrector for MockOk {
        fn corregir_pagina(&self, b: &[u8], _: u32, _: u32) -> Result<(Vec<u8>, OrientationResult), DomainError> {
            Ok((b.to_vec(), OrientationResult::sin_rotacion(0.95, false)))
        }
    }
    impl LayoutDetector for MockOk {
        fn detectar(&self, _: &[u8], _: u32, _: u32, num: u32) -> Result<Vec<Region>, DomainError> {
            Ok(vec![Region::new(
                format!("blk_{}_0", num),
                BlockType::Text,
                BoundingBox::new(0.0, 0.0, 10.0, 10.0),
                0.9,
            )])
        }
    }
    struct MockComposerJob;
    impl PageComposer for MockComposerJob {
        fn componer(&self, p: &Page, _: &[ResolvedBlock]) -> Result<ComposedPage, DomainError> {
            Ok(ComposedPage { numero_pagina: p.numero_pagina, pdf_bytes: vec![], texto_extraido: "".into() })
        }
    }
    struct MockOutputJob;
    impl OutputGenerator for MockOutputJob {
        fn generar(&self, _: &Document, _: &[ComposedPage], _: &str) -> Result<(), DomainError> { Ok(()) }
    }
    struct MockResolverJob;
    impl reconstructor_domain::traits::BlockResolver for MockResolverJob {
        fn puede_resolver(&self, _: BlockType) -> bool { true }
        fn resolver(&self, _: &Region, _: &[u8]) -> Result<(ResolvedContent, f32), DomainError> {
            Ok((ResolvedContent::Text { texto: "ok".into() }, 0.95))
        }
    }

    fn make_orchestrator() -> Arc<PipelineOrchestrator> {
        let (orq, _rx) = PipelineOrchestrator::new(
            Box::new(MockOk),
            Box::new(MockOk),
            ResolverFactory::new(vec![Box::new(MockResolverJob)]),
            Box::new(MockComposerJob),
            vec![Box::new(MockOutputJob)],
            PipelineConfig::default(),
        );
        Arc::new(orq)
    }

    fn pagina_input() -> PageImage {
        PageImage { datos: vec![0u8; 16], ancho: 100, alto: 100, numero_pagina: 1 }
    }

    #[test]
    fn nuevo_job_manager_no_esta_activo() {
        let jm = JobManager::new();
        assert!(!jm.esta_activo());
    }

    #[test]
    fn iniciar_job_pone_activo() {
        let jm = JobManager::new();
        let orq = make_orchestrator();
        jm.iniciar(orq, vec![pagina_input()], "/tmp/test_job".into());
    }

    #[test]
    fn verificar_resultado_retorna_documento() {
        let jm = JobManager::new();
        let orq = make_orchestrator();
        jm.iniciar(orq, vec![pagina_input()], "/tmp/test_job".into());

        let mut intentos = 0;
        let resultado = loop {
            std::thread::sleep(std::time::Duration::from_millis(10));
            if let Some(r) = jm.verificar_resultado() {
                break r;
            }
            intentos += 1;
            if intentos > 100 {
                panic!("El job no termino en tiempo esperado");
            }
        };

        assert!(resultado.is_ok());
        let doc = resultado.unwrap();
        assert_eq!(doc.metricas.total_paginas, 1);
    }

    #[test]
    fn resetear_vuelve_a_inactivo() {
        let jm = JobManager::new();
        let orq = make_orchestrator();
        jm.iniciar(orq, vec![pagina_input()], "/tmp/test_reset".into());
        std::thread::sleep(std::time::Duration::from_millis(50));
        jm.verificar_resultado(); 
        jm.resetear();
        assert!(!jm.esta_activo());
    }
}
