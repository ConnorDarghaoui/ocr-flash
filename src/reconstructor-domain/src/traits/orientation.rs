// =============================================================================
// OrientationCorrector — Abstracción de Corrección Espacial (ADR-008)
//
// Propósito: Ocultar la orquestación de tensores de los modelos rotacionales
//            aislando la lógica del Dominio. Permite ejecutar inferencias de 
//            orientación en hardware acelerado sin acoplarse a librerías específicas.
// =============================================================================

use crate::error::DomainError;

/// Encapsula la predicción espacial y su umbral de certidumbre.
#[derive(Debug, Clone)]
pub struct OrientationResult {
    pub angulo_grados: f32,
    pub confianza: f32,
    pub incierto: bool,
}

impl OrientationResult {
    pub fn sin_rotacion(confianza: f32, incierto: bool) -> Self {
        Self { angulo_grados: 0.0, confianza, incierto }
    }
}

/// Contrato para el alineamiento geométrico de la hoja completa antes del Layout.
///
/// Previene que el detector de regiones asuma orientaciones erróneas que destruirían 
/// el Reading-Order. Se espera que su ejecución sea síncrona por página.
pub trait OrientationCorrector: Send + Sync {
    /// Infiere la inclinación global de un raster e intenta rectificarlo.
    ///
    /// # Arguments
    ///
    /// * `imagen_bytes` - Buffer codificado estático de la captura original.
    /// * `ancho` / `alto` - Dimensiones nominales provistas por el decoder base.
    ///
    /// # Errors
    ///
    /// Retorna `DomainError` si las dimensiones provistas causan un desbordamiento al
    /// alocar memoria en el runtime subyacente.
    fn corregir_pagina(
        &self,
        imagen_bytes: &[u8],
        ancho: u32,
        alto: u32,
    ) -> Result<(Vec<u8>, OrientationResult), DomainError>;
}

/// Contrato para el alineamiento geométrico granular (por línea de texto).
///
/// Optimiza el pipeline de extracción de caracteres permitiendo pasar por un modelo binario
/// (derecho/invertido) un bloque específico, reduciendo errores en diccionarios.
pub trait TextlineOrientationCorrector: Send + Sync {
    /// Ejecuta una inferencia masiva y paralela de múltiples sub-regiones cortadas.
    ///
    /// # Arguments
    ///
    /// * `crops` - Colección de recortes codificados listos para alimentar el batch.
    ///
    /// # Errors
    ///
    /// Propaga un `DomainError` si el encolado supera la capacidad del runtime ML.
    fn corregir_lineas(
        &self,
        crops: &[Vec<u8>],
    ) -> Result<Vec<OrientationResult>, DomainError>;
}
