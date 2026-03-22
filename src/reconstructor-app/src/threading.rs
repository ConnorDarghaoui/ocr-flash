// =============================================================================
// threading — Inicialización del thread pool rayon (F4.4)
//
// Configura el pool global de rayon según `GeneralConfig.num_threads`.
// Debe llamarse una sola vez al inicio del proceso, antes de cualquier
// operación paralela.
// =============================================================================

use reconstructor_domain::GeneralConfig;

/// Inicializa el pool global de rayon con los parámetros de `config`.
///
/// - `num_threads == 0`: usa todos los núcleos lógicos disponibles (comportamiento por defecto de rayon).
/// - `num_threads > 0`: limita el pool al número especificado.
///
/// Si rayon ya fue inicializado (p. ej. en tests), la llamada es silenciosamente ignorada.
pub fn inicializar_thread_pool(config: &GeneralConfig) {
    let num_threads = if config.num_threads == 0 {
        // 0 → rayon usa su heurística por defecto (num_cpus)
        return;
    } else {
        config.num_threads
    };

    match rayon::ThreadPoolBuilder::new()
        .num_threads(num_threads)
        .build_global()
    {
        Ok(()) => {
            tracing::info!(
                "Rayon thread pool inicializado con {} hilos",
                num_threads
            );
        }
        Err(e) => {
            // Ya estaba inicializado (común en tests o si se llama dos veces)
            tracing::debug!("Thread pool ya inicializado: {e}");
        }
    }
}

/// Devuelve la cantidad de hilos que rayon usaría para el config dado.
/// Útil para logging y benchmarks.
pub fn num_threads_efectivos(config: &GeneralConfig) -> usize {
    if config.num_threads == 0 {
        rayon::current_num_threads()
    } else {
        config.num_threads
    }
}
