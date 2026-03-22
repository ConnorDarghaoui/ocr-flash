// =============================================================================
// RasterFallbackResolver — Barrera de degradación segura (Catch-all)
//
// Propósito: Interceptar inferencias fallidas o bloques lógicamente irresolubles
//            (como imágenes y gráficos). Actúa como último eslabón en la Cadena 
//            de Responsabilidad (`ResolverFactory`) retornando incondicionalmente
//            el buffer gráfico subyacente.
// =============================================================================

use reconstructor_domain::traits::BlockResolver;
use reconstructor_domain::{BlockType, DomainError, Region, ResolvedContent};

/// Implementación nula ("Null Object" enriquecido) para la abstracción de resolución.
///
/// Previene la detención de la Máquina de Estados (BlockFSM) inyectando una
/// transición forzada a `Resolved` asumiendo las matrices de píxeles como contenido
/// primario de valor.
pub struct RasterFallbackResolver;

impl BlockResolver for RasterFallbackResolver {
    /// Omitir la validación de semántica y absorber cualquier solicitud.
    fn puede_resolver(&self, _tipo: BlockType) -> bool {
        true
    }

    /// Extrapola las dimensiones directamente del header del binario para esquivar
    /// el overhead de CPU/memoria de invocar bibliotecas gráficas pesadas (como `image-rs`).
    ///
    /// # Arguments
    ///
    /// * `_region` - Metadato gráfico (Ignorado).
    /// * `crop_bytes` - Buffer de bytes estático conteniendo el PNG codificado en memoria.
    fn resolver(
        &self,
        _region: &Region,
        crop_bytes: &[u8],
    ) -> Result<(ResolvedContent, f32), DomainError> {
        let (ancho, alto) = dimensiones_png(crop_bytes);
        Ok((
            ResolvedContent::Raster {
                imagen_bytes: crop_bytes.to_vec(),
                ancho,
                alto,
            },
            1.0,
        ))
    }
}

fn dimensiones_png(bytes: &[u8]) -> (u32, u32) {
    if bytes.len() >= 24
        && bytes[0] == 137
        && bytes[1] == 80
        && bytes[2] == 78
        && bytes[3] == 71
    {
        let ancho =
            u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let alto =
            u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        (ancho, alto)
    } else {
        (0, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reconstructor_domain::{BoundingBox, BlockType, Region};
    use std::io::Cursor;

    fn region_figura() -> Region {
        Region::new(
            "blk_1_0",
            BlockType::Figure,
            BoundingBox::new(0.0, 0.0, 100.0, 100.0),
            0.9,
        )
    }

    fn png_sintetico(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbImage::new(width, height);
        let dyn_img = image::DynamicImage::ImageRgb8(img);
        let mut bytes = Vec::new();
        dyn_img
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .expect("encode PNG");
        bytes
    }

    #[test]
    fn puede_resolver_siempre_true() {
        let r = RasterFallbackResolver;
        assert!(r.puede_resolver(BlockType::Figure));
        assert!(r.puede_resolver(BlockType::Text));
        assert!(r.puede_resolver(BlockType::Table));
        assert!(r.puede_resolver(BlockType::Unknown));
    }

    #[test]
    fn resolver_bytes_vacios_retorna_dimensiones_cero() {
        let r = RasterFallbackResolver;
        let region = region_figura();
        let (content, conf) = r.resolver(&region, &[]).unwrap();
        assert_eq!(conf, 1.0);
        match content {
            ResolvedContent::Raster { imagen_bytes, ancho, alto } => {
                assert!(imagen_bytes.is_empty());
                assert_eq!(ancho, 0);
                assert_eq!(alto, 0);
            }
            _ => panic!("se esperaba Raster"),
        }
    }

    #[test]
    fn resolver_png_valido_extrae_dimensiones() {
        let r = RasterFallbackResolver;
        let region = region_figura();
        let png = png_sintetico(64, 32);
        let (content, conf) = r.resolver(&region, &png).unwrap();
        assert_eq!(conf, 1.0);
        match content {
            ResolvedContent::Raster { ancho, alto, .. } => {
                assert_eq!(ancho, 64);
                assert_eq!(alto, 32);
            }
            _ => panic!("se esperaba Raster"),
        }
    }
}
