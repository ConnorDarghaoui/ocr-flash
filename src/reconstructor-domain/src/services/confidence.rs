// =============================================================================
// ConfidenceEvaluator — Árbitro de heurísticas de calidad
//
// Propósito: Desacopla la lógica de evaluación (umbrales y caracteres) de las 
//            transiciones puras del BlockFSM. Permite inyectar reglas de negocio 
//            dinámicas (ej. tolerancias configurables por el usuario).
//
// Flujo:     |> Evaluación de entropía léxica (caracteres de reemplazo)
//            |> Evaluación de confianza estadística del modelo
//            |> Mapeo a decisión semántica
// =============================================================================

/// Decisión de calidad que dictamina la siguiente transición del autómata (BlockFSM).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfidenceDecision {
    Alta,
    Moderada,
    /// Fuerza al autómata a transicionar a reintentos o fallback raster.
    Baja,
}

/// Entidad responsable de auditar los resultados de inferencia en función de parámetros de negocio.
#[derive(Debug, Clone)]
pub struct ConfidenceEvaluator {
    pub threshold: f32,
    pub max_unrecognizable_ratio: f32,
}

impl ConfidenceEvaluator {
    /// Inyecta los umbrales de configuración requeridos para operar.
    pub fn new(threshold: f32, max_unrecognizable_ratio: f32) -> Self {
        Self { threshold, max_unrecognizable_ratio }
    }

    /// Determina la viabilidad del resultado de una inferencia ML basándose en la sección 9.1 del SRS.
    ///
    /// Se prioriza la heurística léxica sobre el `score` matemático del modelo, porque un motor OCR 
    /// puede asignar una "confianza alta" a una secuencia repetitiva de caracteres `U+FFFD` si el 
    /// decodificador interno colapsa.
    ///
    /// # Arguments
    ///
    /// * `confianza` - La probabilidad `[0.0, 1.0]` generada por la capa softmax del modelo.
    /// * `texto` - La cadena de caracteres decodificada a auditar.
    pub fn evaluar(&self, confianza: f32, texto: &str) -> ConfidenceDecision {
        if !texto.is_empty() && self.ratio_caracteres_invalidos(texto) > self.max_unrecognizable_ratio {
            return ConfidenceDecision::Baja;
        }

        if confianza >= 0.80 {
            ConfidenceDecision::Alta
        } else if confianza >= self.threshold {
            ConfidenceDecision::Moderada
        } else {
            ConfidenceDecision::Baja
        }
    }

    fn ratio_caracteres_invalidos(&self, texto: &str) -> f32 {
        let total = texto.chars().count();
        if total == 0 {
            return 0.0;
        }
        let invalidos = texto.chars().filter(|c| Self::es_invalido(*c)).count();
        invalidos as f32 / total as f32
    }

    fn es_invalido(c: char) -> bool {
        c == '\u{FFFD}'
            || (c.is_control() && !matches!(c, '\n' | '\r' | '\t' | ' '))
    }
}

impl Default for ConfidenceEvaluator {
    fn default() -> Self {
        Self::new(0.60, 0.30)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evaluador() -> ConfidenceEvaluator {
        ConfidenceEvaluator::new(0.60, 0.30)
    }

    #[test]
    fn confianza_alta_returna_alta() {
        assert_eq!(evaluador().evaluar(0.95, "texto limpio"), ConfidenceDecision::Alta);
    }

    #[test]
    fn confianza_exactamente_08_returna_alta() {
        assert_eq!(evaluador().evaluar(0.80, "texto"), ConfidenceDecision::Alta);
    }

    #[test]
    fn confianza_moderada_returna_moderada() {
        assert_eq!(evaluador().evaluar(0.70, "texto"), ConfidenceDecision::Moderada);
    }

    #[test]
    fn confianza_exactamente_en_threshold_returna_moderada() {
        assert_eq!(evaluador().evaluar(0.60, "texto"), ConfidenceDecision::Moderada);
    }

    #[test]
    fn confianza_baja_returna_baja() {
        assert_eq!(evaluador().evaluar(0.40, "texto"), ConfidenceDecision::Baja);
    }

    #[test]
    fn confianza_cero_returna_baja() {
        assert_eq!(evaluador().evaluar(0.0, "texto"), ConfidenceDecision::Baja);
    }

    #[test]
    fn texto_con_muchos_replacement_chars_fuerza_baja_aunque_confianza_alta() {
        let texto = "\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}texto";
        assert_eq!(evaluador().evaluar(0.95, texto), ConfidenceDecision::Baja);
    }

    #[test]
    fn texto_con_pocos_replacement_chars_no_activa_heuristica() {
        let texto = "texto limpio con un \u{FFFD}";
        assert_eq!(evaluador().evaluar(0.90, texto), ConfidenceDecision::Alta);
    }

    #[test]
    fn texto_vacio_no_activa_heuristica() {
        assert_eq!(evaluador().evaluar(0.85, ""), ConfidenceDecision::Alta);
    }

    #[test]
    fn texto_solo_whitespace_no_cuenta_como_invalido() {
        let texto = "linea 1\nlinea 2\tcon tab";
        assert_eq!(evaluador().evaluar(0.90, texto), ConfidenceDecision::Alta);
    }

    #[test]
    fn threshold_alto_clasifica_mas_como_baja() {
        let eval = ConfidenceEvaluator::new(0.85, 0.30);
        assert_eq!(eval.evaluar(0.75, "texto"), ConfidenceDecision::Baja);
    }

    #[test]
    fn threshold_bajo_clasifica_mas_como_moderada() {
        let eval = ConfidenceEvaluator::new(0.40, 0.30);
        assert_eq!(eval.evaluar(0.50, "texto"), ConfidenceDecision::Moderada);
    }
}
