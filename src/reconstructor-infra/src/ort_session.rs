// =============================================================================
// ort_session — Helper compartido para construir sesiones ONNX con GPU fallback
// =============================================================================

use ort::session::Session;

/// Construye una sesión ONNX con el proveedor de ejecución adecuado.
///
/// Si `use_gpu = true`, intenta CUDA → DirectML → CoreML → CPU (en ese orden).
/// Si el EP preferido no está disponible, ort cae automáticamente a CPU.
/// Nunca falla por ausencia de GPU.
pub fn construir_sesion(model_path: &str, use_gpu: bool) -> Result<Session, ort::Error> {
    let mut builder = Session::builder()?;
    if use_gpu {
        builder
            .with_execution_providers([
                ort::execution_providers::CUDAExecutionProvider::default().build(),
                ort::execution_providers::DirectMLExecutionProvider::default().build(),
                ort::execution_providers::CoreMLExecutionProvider::default().build(),
            ])?
            .commit_from_file(model_path)
    } else {
        builder.commit_from_file(model_path)
    }
}
