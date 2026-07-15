use std::{fs, path::PathBuf};

use mdconvert_core::{Block, ConversionError, ConversionRequest, Converter, WarningCode};
use mdconvert_formats::ImageConverter;
use tempfile::TempDir;

fn request(path: impl Into<PathBuf>) -> ConversionRequest {
    ConversionRequest::new(path).unwrap()
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

fn png_chunk(output: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    output.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
    output.extend_from_slice(kind);
    output.extend_from_slice(data);
    let mut crc_input = kind.to_vec();
    crc_input.extend_from_slice(data);
    output.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn png(width: u32, height: u32, text: Option<(&str, &str)>) -> Vec<u8> {
    let mut output = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    png_chunk(&mut output, b"IHDR", &ihdr);
    if let Some((key, value)) = text {
        let mut data = key.as_bytes().to_vec();
        data.push(0);
        data.extend_from_slice(value.as_bytes());
        png_chunk(&mut output, b"tEXt", &data);
    }
    // Valid zlib stream for one transparent RGBA scanline when width is one.
    png_chunk(
        &mut output,
        b"IDAT",
        &[0x78, 0x9c, 0x63, 0x60, 0x60, 0x60, 0, 0, 0, 5, 0, 1],
    );
    png_chunk(&mut output, b"IEND", &[]);
    output
}

fn jpeg(width: u16, height: u16) -> Vec<u8> {
    let mut output = vec![0xff, 0xd8];
    let comment = b"Description from JPEG";
    output.extend_from_slice(&[0xff, 0xfe]);
    output.extend_from_slice(&u16::try_from(comment.len() + 2).unwrap().to_be_bytes());
    output.extend_from_slice(comment);
    output.extend_from_slice(&[0xff, 0xc0, 0, 17, 8]);
    output.extend_from_slice(&height.to_be_bytes());
    output.extend_from_slice(&width.to_be_bytes());
    output.extend_from_slice(&[3, 1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0]);
    output.extend_from_slice(&[0xff, 0xd9]);
    output
}

#[test]
fn png_dimensions_and_semantic_metadata_are_preserved_without_ocr() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("metadata.png");
    fs::write(&path, png(1, 1, Some(("Title", "Diagram label")))).unwrap();

    let document = ImageConverter.convert(&request(path)).unwrap();
    assert_eq!(document.metadata.source_format.as_deref(), Some("png"));
    assert_eq!(document.metadata.properties.get("width"), Some(&"1".into()));
    assert_eq!(
        document.metadata.properties.get("height"),
        Some(&"1".into())
    );
    assert_eq!(
        document.metadata.properties.get("png.Title"),
        Some(&"Diagram label".into())
    );
    assert_eq!(document.assets.len(), 1);
    assert!(matches!(document.blocks.as_slice(), [Block::Image { .. }]));
    assert!(
        !document
            .warnings
            .iter()
            .any(|warning| warning.code == WarningCode::OcrDeferred)
    );
}

#[test]
fn jpeg_dimensions_and_comment_metadata_are_preserved_without_decoding_pixels() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("metadata.jpg");
    fs::write(&path, jpeg(320, 200)).unwrap();

    let document = ImageConverter.convert(&request(path)).unwrap();
    assert_eq!(document.metadata.source_format.as_deref(), Some("jpeg"));
    assert_eq!(
        document.metadata.properties.get("width"),
        Some(&"320".into())
    );
    assert_eq!(
        document.metadata.properties.get("height"),
        Some(&"200".into())
    );
    assert_eq!(
        document.metadata.properties.get("jpeg.comment"),
        Some(&"Description from JPEG".into())
    );
    assert!(
        !document
            .warnings
            .iter()
            .any(|warning| warning.code == WarningCode::OcrDeferred)
    );
}

#[test]
fn image_with_only_pixel_text_defers_ocr_instead_of_calling_or_requiring_it() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("pixels-contain-text.png");
    fs::write(&path, png(1, 1, None)).unwrap();

    let result = ImageConverter.convert(&request(path));
    assert!(!matches!(result, Err(ConversionError::OcrRequired)));
    let document = result.unwrap();
    assert!(document.warnings.iter().any(|warning| {
        warning.code == WarningCode::OcrDeferred
            && warning.message.contains("OCR was not run")
            && warning.page.is_none()
    }));
}

#[test]
fn corrupt_png_crc_is_rejected() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("corrupt.png");
    let mut bytes = png(1, 1, None);
    bytes[29] ^= 1;
    fs::write(&path, bytes).unwrap();

    assert!(matches!(
        ImageConverter.convert(&request(path)),
        Err(ConversionError::CorruptInput { .. })
    ));
}

#[test]
fn uncompressed_png_international_text_is_bounded_and_preserved() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("international.png");
    let mut bytes = png(1, 1, None);
    let insertion = bytes
        .windows(4)
        .position(|window| window == b"IDAT")
        .unwrap()
        - 4;
    let mut chunk = Vec::new();
    let mut data = b"Description\0".to_vec();
    data.extend_from_slice(&[0, 0]);
    data.push(0);
    data.push(0);
    data.extend_from_slice("Texto semántico".as_bytes());
    png_chunk(&mut chunk, b"iTXt", &data);
    bytes.splice(insertion..insertion, chunk);
    fs::write(&path, bytes).unwrap();

    let document = ImageConverter.convert(&request(path)).unwrap();
    assert_eq!(
        document.metadata.properties.get("png.Description"),
        Some(&"Texto semántico".into())
    );
}
