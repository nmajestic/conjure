use std::io::Cursor;

use crate::error::ExportError;

pub struct DecodedPng {
    pub width: u32,
    pub height: u32,
    pub palette: Vec<[u8; 3]>,
    pub trns: Option<Vec<u8>>,
    pub indices: Vec<u8>,
}

pub fn encode_indexed_png(
    width: u32,
    height: u32,
    palette: &[[u8; 3]],
    indices: &[u8],
) -> Result<Vec<u8>, ExportError> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Indexed);
    encoder.set_depth(png::BitDepth::Eight);
    let flat: Vec<u8> = palette.iter().flat_map(|t| t.iter().copied()).collect();
    encoder.set_palette(flat);
    encoder.set_trns(vec![0u8]);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(indices)?;
    writer.finish()?;
    Ok(out)
}

pub fn decode_indexed_png(bytes: &[u8]) -> Result<DecodedPng, ExportError> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::IDENTITY);
    let mut reader = decoder.read_info()?;
    let info = reader.info();
    let palette: Vec<[u8; 3]> = match info.palette.as_ref() {
        Some(raw) => {
            let (triples, _) = raw.as_chunks::<3>();
            triples.to_vec()
        }
        None => return Err(ExportError::Invalid("png has no palette".to_string())),
    };
    let trns = info.trns.as_ref().map(|t| t.to_vec());
    let width = info.width;
    let height = info.height;
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| ExportError::Invalid("png too large to decode".to_string()))?;
    let mut buf = vec![0u8; size];
    reader.next_frame(&mut buf)?;
    Ok(DecodedPng {
        width,
        height,
        palette,
        trns,
        indices: buf,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // U9.1: encoded output starts with the PNG signature bytes.
    #[test]
    fn encoded_png_starts_with_signature() {
        let palette = [[0u8, 0, 0], [1, 2, 3], [9, 9, 9]];
        let bytes = encode_indexed_png(3, 2, &palette, &[0, 1, 2, 2, 1, 0]).unwrap();
        assert_eq!(&bytes[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
    }

    // U9.2: decoding a PNG without a PLTE chunk is refused.
    #[test]
    fn decode_rejects_png_without_palette() {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, 2, 1);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[0, 0]).unwrap();
        writer.finish().unwrap();
        let err = match decode_indexed_png(&out) {
            Err(e) => e,
            Ok(_) => panic!("expected Invalid(\"png has no palette\"), got Ok"),
        };
        assert!(matches!(err, ExportError::Invalid(ref m) if m == "png has no palette"));
    }

    // U9.3: roundtrip preserves indices, palette order, and tRNS.
    #[test]
    fn roundtrip_preserves_indices_palette_and_trns() {
        let palette = [[0u8, 0, 0], [1, 2, 3], [9, 9, 9]];
        let indices = [0u8, 1, 2, 2, 1, 0];
        let bytes = encode_indexed_png(3, 2, &palette, &indices).unwrap();
        let decoded = decode_indexed_png(&bytes).unwrap();
        assert_eq!(decoded.width, 3);
        assert_eq!(decoded.height, 2);
        assert_eq!(decoded.palette, palette.to_vec());
        assert_eq!(decoded.trns, Some(vec![0u8]));
        assert_eq!(decoded.indices, indices.to_vec());
    }

    // U9.4: a 5x3 pattern of all-distinct indices decodes position-exact.
    #[test]
    fn distinct_positions_preserved() {
        let palette: Vec<[u8; 3]> = (0..15u8).map(|i| [i, i, i]).collect();
        let indices: Vec<u8> = (0..15u8).collect();
        let bytes = encode_indexed_png(5, 3, &palette, &indices).unwrap();
        let decoded = decode_indexed_png(&bytes).unwrap();
        assert_eq!(decoded.width, 5);
        assert_eq!(decoded.height, 3);
        assert_eq!(decoded.indices, indices);
    }
}
