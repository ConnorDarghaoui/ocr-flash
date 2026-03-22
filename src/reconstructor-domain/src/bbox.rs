// =============================================================================
// BoundingBox — Representación geométrica espacial escalar
//
// Propósito: Define la inmutabilidad de las coordenadas base (siempre en pixeles
//            nativos de la captura original). Delega cualquier transformación 
//            (como el escalado a puntos tipográficos PDF) a los generadores 
//            terminales de la infraestructura.
// =============================================================================

use serde::{Deserialize, Serialize};

/// Delimitador espacial de una inferencia visual (Layout u OCR).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl BoundingBox {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width, height }
    }

    pub fn area(&self) -> f32 {
        self.width * self.height
    }

    pub fn derecho(&self) -> f32 {
        self.x + self.width
    }

    pub fn inferior(&self) -> f32 {
        self.y + self.height
    }

    pub fn centro(&self) -> (f32, f32) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    /// Calcula la Intersección sobre Unión (IoU).
    ///
    /// Algoritmo base requerido por el módulo de Layout para ejecutar el 
    /// filtrado Non-Maximum Suppression (NMS) sobre tensores solapados,
    /// previniendo la duplicación de bloques de lectura.
    ///
    /// # Arguments
    ///
    /// * `otro` - Instancia espacial a comparar.
    pub fn iou(&self, otro: &BoundingBox) -> f32 {
        let interseccion_x = self.x.max(otro.x);
        let interseccion_y = self.y.max(otro.y);
        let interseccion_derecho = self.derecho().min(otro.derecho());
        let interseccion_inferior = self.inferior().min(otro.inferior());

        let ancho_interseccion = (interseccion_derecho - interseccion_x).max(0.0);
        let alto_interseccion = (interseccion_inferior - interseccion_y).max(0.0);
        let area_interseccion = ancho_interseccion * alto_interseccion;

        let area_union = self.area() + otro.area() - area_interseccion;

        if area_union <= 0.0 {
            return 0.0;
        }

        area_interseccion / area_union
    }

    /// Comprueba la contención estricta para resolver jerarquías espaciales
    /// (ej. sub-bloques detectados anómalamente dentro de una Tabla mayor).
    pub fn contiene(&self, otro: &BoundingBox) -> bool {
        self.x <= otro.x
            && self.y <= otro.y
            && self.derecho() >= otro.derecho()
            && self.inferior() >= otro.inferior()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_area_calculo_correcto() {
        let bbox = BoundingBox::new(10.0, 20.0, 100.0, 50.0);
        assert_eq!(bbox.area(), 5000.0);
    }

    #[test]
    fn test_centro_calculo_correcto() {
        let bbox = BoundingBox::new(0.0, 0.0, 100.0, 200.0);
        assert_eq!(bbox.centro(), (50.0, 100.0));
    }

    #[test]
    fn test_iou_identicos() {
        let bbox = BoundingBox::new(0.0, 0.0, 100.0, 100.0);
        let iou = bbox.iou(&bbox);
        assert!((iou - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_iou_sin_interseccion() {
        let a = BoundingBox::new(0.0, 0.0, 50.0, 50.0);
        let b = BoundingBox::new(100.0, 100.0, 50.0, 50.0);
        assert_eq!(a.iou(&b), 0.0);
    }

    #[test]
    fn test_iou_interseccion_parcial() {
        let a = BoundingBox::new(0.0, 0.0, 100.0, 100.0);
        let b = BoundingBox::new(50.0, 50.0, 100.0, 100.0);
        let esperado = 2500.0 / 17500.0;
        assert!((a.iou(&b) - esperado).abs() < 1e-5);
    }

    #[test]
    fn test_contiene() {
        let grande = BoundingBox::new(0.0, 0.0, 200.0, 200.0);
        let pequeno = BoundingBox::new(10.0, 10.0, 50.0, 50.0);
        assert!(grande.contiene(&pequeno));
        assert!(!pequeno.contiene(&grande));
    }
}
