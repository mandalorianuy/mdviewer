use std::{fs, path::PathBuf};

use mdconvert_core::{Block, ConversionError, ConversionRequest, Converter, WarningCode};
use mdconvert_formats::{ImageConverter, ImageLimits};
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
        &[0x78, 0x9c, 0x63, 0x60, 0, 2, 0, 0, 5, 0, 1],
    );
    png_chunk(&mut output, b"IEND", &[]);
    output
}

fn png_with_chunks(
    bit_depth: u8,
    color_type: u8,
    interlace: u8,
    palettes: &[&[u8]],
    transparency: Option<&[u8]>,
    compressed: &[u8],
) -> Vec<u8> {
    let mut output = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&[bit_depth, color_type, 0, 0, interlace]);
    png_chunk(&mut output, b"IHDR", &ihdr);
    for palette in palettes {
        png_chunk(&mut output, b"PLTE", palette);
    }
    if let Some(transparency) = transparency {
        png_chunk(&mut output, b"tRNS", transparency);
    }
    png_chunk(&mut output, b"IDAT", compressed);
    png_chunk(&mut output, b"IEND", &[]);
    output
}

fn truecolor_png_with_palette_and_transparency(palette_first: bool) -> Vec<u8> {
    let mut output = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    png_chunk(&mut output, b"IHDR", &ihdr);
    if palette_first {
        png_chunk(&mut output, b"PLTE", &[0, 0, 0]);
        png_chunk(&mut output, b"tRNS", &[0, 0, 0, 0, 0, 0]);
    } else {
        png_chunk(&mut output, b"tRNS", &[0, 0, 0, 0, 0, 0]);
        png_chunk(&mut output, b"PLTE", &[0, 0, 0]);
    }
    png_chunk(
        &mut output,
        b"IDAT",
        &[0x78, 0x9c, 0x63, 0x60, 0x60, 0x60, 0, 0, 0, 4, 0, 1],
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
    output.extend_from_slice(&[0xff, 0xda, 0, 12, 3, 1, 0, 2, 0, 3, 0, 0, 63, 0, 0]);
    output.extend_from_slice(&[0xff, 0xd9]);
    output
}

fn multiscan_jpeg() -> Vec<u8> {
    let mut output = vec![0xff, 0xd8, 0xff, 0xc0, 0, 17, 8, 0, 1, 0, 1, 3];
    output.extend_from_slice(&[1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0]);
    for component in [1, 2, 3] {
        output.extend_from_slice(&[0xff, 0xda, 0, 8, 1, component, 0, 0, 63, 0, 0]);
    }
    output.extend_from_slice(&[0xff, 0xd9]);
    output
}

fn jpeg_with_restarts() -> Vec<u8> {
    let mut bytes = jpeg(1, 1);
    let sos = bytes
        .windows(2)
        .position(|window| window == [0xff, 0xda])
        .unwrap();
    bytes.splice(sos..sos, [0xff, 0xdd, 0, 4, 0, 1]);
    let eoi = bytes
        .windows(2)
        .position(|window| window == [0xff, 0xd9])
        .unwrap();
    bytes.splice(eoi - 1..eoi, [0, 0xff, 0xd0, 0, 0xff, 0xd1, 0]);
    bytes
}

fn multiscan_jpeg_with_redefined_dri(restart_after_disable: bool) -> Vec<u8> {
    let mut output = vec![0xff, 0xd8, 0xff, 0xc0, 0, 17, 8, 0, 8, 0, 24, 3];
    output.extend_from_slice(&[1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0]);
    output.extend_from_slice(&[0xff, 0xdd, 0, 4, 0, 1]);
    output.extend_from_slice(&[
        0xff, 0xda, 0, 8, 1, 1, 0, 0, 63, 0, 0, 0xff, 0xd0, 0, 0xff, 0xd1, 0,
    ]);
    output.extend_from_slice(&[0xff, 0xdd, 0, 4, 0, 2]);
    output.extend_from_slice(&[0xff, 0xda, 0, 8, 1, 2, 0, 0, 63, 0, 0, 0, 0xff, 0xd0, 0]);
    output.extend_from_slice(&[0xff, 0xdd, 0, 4, 0, 0]);
    output.extend_from_slice(&[0xff, 0xda, 0, 8, 1, 3, 0, 0, 63, 0, 0]);
    if restart_after_disable {
        output.extend_from_slice(&[0xff, 0xd0, 0]);
    }
    output.extend_from_slice(&[0xff, 0xd9]);
    output
}

#[test]
fn image_dimensions_and_pixel_count_are_checked_before_asset_allocation() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("oversize.png");
    fs::write(&path, png(100, 100, None)).unwrap();

    assert!(matches!(
        ImageConverter::with_limits(ImageLimits {
            max_width: 50,
            max_height: 100,
            max_pixels: 5_000,
            ..ImageLimits::default()
        })
        .convert(&request(path)),
        Err(ConversionError::LimitExceeded {
            limit: "image_width",
            actual: 100,
            maximum: 50,
        })
    ));
}

#[test]
fn jpeg_requires_a_frame_scan_entropy_and_terminal_eoi() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("no-scan.jpg");
    let mut bytes = jpeg(1, 1);
    let scan = bytes
        .windows(2)
        .position(|window| window == [0xff, 0xda])
        .unwrap();
    bytes.splice(scan.., [0xff, 0xd9]);
    fs::write(&path, bytes).unwrap();
    assert!(matches!(
        ImageConverter.convert(&request(path)),
        Err(ConversionError::CorruptInput { .. })
    ));

    let truncated = temp.path().join("truncated-scan.jpg");
    let mut bytes = jpeg(1, 1);
    bytes.truncate(bytes.len() - 2);
    fs::write(&truncated, bytes).unwrap();
    assert!(matches!(
        ImageConverter.convert(&request(truncated)),
        Err(ConversionError::CorruptInput { .. })
    ));
}

#[test]
fn jpeg_validates_frame_components_scan_parameters_and_multiscan_state() {
    let temp = TempDir::new().unwrap();
    let mut cases = Vec::new();

    let mut bad_components = jpeg(1, 1);
    let sof = bad_components
        .windows(2)
        .position(|window| window == [0xff, 0xc0])
        .unwrap();
    bad_components[sof + 9] = 1;
    cases.push(("sof-components", bad_components));

    let mut bad_selector = jpeg(1, 1);
    let sos = bad_selector
        .windows(2)
        .position(|window| window == [0xff, 0xda])
        .unwrap();
    bad_selector[sos + 5] = 9;
    cases.push(("sos-selector", bad_selector));

    let mut bad_spectral = jpeg(1, 1);
    let sos = bad_spectral
        .windows(2)
        .position(|window| window == [0xff, 0xda])
        .unwrap();
    bad_spectral[sos + 12] = 62;
    cases.push(("sos-spectral", bad_spectral));

    let mut bad_fill = jpeg(1, 1);
    let eoi = bad_fill
        .windows(2)
        .position(|window| window == [0xff, 0xd9])
        .unwrap();
    bad_fill.splice(eoi - 1..eoi, [0xff, 0xff, 0x00]);
    cases.push(("entropy-fill-before-stuffing", bad_fill));

    let progressive_refinement_without_initial = vec![
        0xff, 0xd8, 0xff, 0xc2, 0, 11, 8, 0, 1, 0, 1, 1, 1, 0x11, 0, 0xff, 0xda, 0, 8, 1, 1, 0, 0,
        0, 0x21, 0, 0xff, 0xd9,
    ];
    cases.push((
        "progressive-refinement-without-initial-scan",
        progressive_refinement_without_initial,
    ));

    let mut restart_without_dri = jpeg(1, 1);
    let eoi = restart_without_dri
        .windows(2)
        .position(|window| window == [0xff, 0xd9])
        .unwrap();
    restart_without_dri.splice(eoi - 1..eoi, [0, 0xff, 0xd0, 0]);
    cases.push(("restart-without-dri", restart_without_dri));

    let mut malformed_dri = jpeg(1, 1);
    let sos = malformed_dri
        .windows(2)
        .position(|window| window == [0xff, 0xda])
        .unwrap();
    malformed_dri.splice(sos..sos, [0xff, 0xdd, 0, 3, 1]);
    cases.push(("malformed-dri", malformed_dri));

    cases.push((
        "restart-after-dri-disable",
        multiscan_jpeg_with_redefined_dri(true),
    ));

    for (name, bytes) in cases {
        let path = temp.path().join(format!("{name}.jpg"));
        fs::write(&path, bytes).unwrap();
        assert!(matches!(
            ImageConverter.convert(&request(path)),
            Err(ConversionError::CorruptInput { .. })
        ));
    }

    let valid = temp.path().join("multiscan.jpg");
    fs::write(&valid, multiscan_jpeg()).unwrap();
    assert!(ImageConverter.convert(&request(valid)).is_ok());

    let valid_restarts = temp.path().join("restarts.jpg");
    fs::write(&valid_restarts, jpeg_with_restarts()).unwrap();
    assert!(ImageConverter.convert(&request(valid_restarts)).is_ok());

    let mut disabled = jpeg(1, 1);
    let sos = disabled
        .windows(2)
        .position(|window| window == [0xff, 0xda])
        .unwrap();
    disabled.splice(sos..sos, [0xff, 0xdd, 0, 4, 0, 0]);
    let valid_disabled = temp.path().join("dri-disabled.jpg");
    fs::write(&valid_disabled, disabled).unwrap();
    assert!(ImageConverter.convert(&request(valid_disabled)).is_ok());

    let valid_redefined = temp.path().join("dri-redefined-between-scans.jpg");
    fs::write(&valid_redefined, multiscan_jpeg_with_redefined_dri(false)).unwrap();
    assert!(ImageConverter.convert(&request(valid_redefined)).is_ok());
}

#[test]
fn jpeg_dri_must_be_followed_by_a_frame_or_scan_header() {
    let temp = TempDir::new().unwrap();

    let mut before_eoi = jpeg(1, 1);
    let eoi = before_eoi
        .windows(2)
        .position(|window| window == [0xff, 0xd9])
        .unwrap();
    before_eoi.splice(eoi..eoi, [0xff, 0xdd, 0, 4, 0, 1]);
    let invalid_final = temp.path().join("dri-before-eoi.jpg");
    fs::write(&invalid_final, before_eoi).unwrap();
    assert!(matches!(
        ImageConverter.convert(&request(invalid_final)),
        Err(ConversionError::CorruptInput { .. })
    ));

    let mut comment_before_eoi = jpeg(1, 1);
    let eoi = comment_before_eoi
        .windows(2)
        .position(|window| window == [0xff, 0xd9])
        .unwrap();
    comment_before_eoi.splice(eoi..eoi, [0xff, 0xdd, 0, 4, 0, 1, 0xff, 0xfe, 0, 3, b'x']);
    let invalid_terminal = temp.path().join("dri-comment-before-eoi.jpg");
    fs::write(&invalid_terminal, comment_before_eoi).unwrap();
    assert!(matches!(
        ImageConverter.convert(&request(invalid_terminal)),
        Err(ConversionError::CorruptInput { .. })
    ));

    let mut before_frame = jpeg(1, 1);
    let frame = before_frame
        .windows(2)
        .position(|window| window == [0xff, 0xc0])
        .unwrap();
    before_frame.splice(frame..frame, [0xff, 0xdd, 0, 4, 0, 0]);
    let valid_frame = temp.path().join("dri-before-frame.jpg");
    fs::write(&valid_frame, before_frame).unwrap();
    assert!(ImageConverter.convert(&request(valid_frame)).is_ok());

    let mut tables_before_scan = jpeg(1, 1);
    let scan = tables_before_scan
        .windows(2)
        .position(|window| window == [0xff, 0xda])
        .unwrap();
    tables_before_scan.splice(scan..scan, [0xff, 0xdd, 0, 4, 0, 0, 0xff, 0xfe, 0, 3, b'x']);
    let valid_scan = temp.path().join("dri-tables-before-scan.jpg");
    fs::write(&valid_scan, tables_before_scan).unwrap();
    assert!(ImageConverter.convert(&request(valid_scan)).is_ok());

    let progressive = [
        0xff, 0xd8, 0xff, 0xc2, 0, 11, 8, 0, 1, 0, 1, 1, 1, 0x11, 0, 0xff, 0xdd, 0, 4, 0, 0, 0xff,
        0xda, 0, 8, 1, 1, 0, 0, 0, 0, 0, 0xff, 0xd9,
    ];
    let valid_progressive = temp.path().join("dri-before-progressive-scan.jpg");
    fs::write(&valid_progressive, progressive).unwrap();
    assert!(ImageConverter.convert(&request(valid_progressive)).is_ok());
}

#[test]
fn technical_metadata_does_not_suppress_ocr_deferred() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("software-only.png");
    fs::write(&path, png(1, 1, Some(("Software", "Camera Tool")))).unwrap();

    let document = ImageConverter.convert(&request(path)).unwrap();
    assert!(
        document
            .warnings
            .iter()
            .any(|warning| warning.code == WarningCode::OcrDeferred)
    );
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
    assert_eq!(
        document.metadata.properties.get("png.interlace_profile"),
        Some(&"non_interlaced_only".into())
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
fn png_requires_a_valid_bounded_image_data_stream() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("invalid-idat.png");
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    png_chunk(&mut bytes, b"IHDR", &ihdr);
    png_chunk(&mut bytes, b"IDAT", b"not-a-zlib-stream");
    png_chunk(&mut bytes, b"IEND", &[]);
    fs::write(&path, bytes).unwrap();

    assert!(matches!(
        ImageConverter.convert(&request(path)),
        Err(ConversionError::CorruptInput { .. })
    ));

    let split = temp.path().join("noncontiguous-idat.png");
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    png_chunk(&mut bytes, b"IHDR", &ihdr);
    let compressed = [0x78, 0x9c, 0x63, 0x60, 0, 2, 0, 0, 5, 0, 1];
    png_chunk(&mut bytes, b"IDAT", &compressed[..6]);
    png_chunk(&mut bytes, b"tEXt", b"Comment\0separator");
    png_chunk(&mut bytes, b"IDAT", &compressed[6..]);
    png_chunk(&mut bytes, b"IEND", &[]);
    fs::write(&split, bytes).unwrap();
    assert!(matches!(
        ImageConverter.convert(&request(split)),
        Err(ConversionError::CorruptInput { .. })
    ));
}

#[test]
fn png_validates_filters_palette_and_noninterlaced_profile() {
    let temp = TempDir::new().unwrap();
    let rgba = [0x78, 0x9c, 0x63, 0x60, 0, 2, 0, 0, 5, 0, 1];
    let bad_filter = [0x78, 0x9c, 0x63, 0x65, 0, 2, 0, 0, 0x1e, 0, 6];
    let indexed = [0x78, 0x9c, 0x63, 0x60, 0, 0, 0, 2, 0, 1];
    let cases = [
        ("filter", png_with_chunks(8, 6, 0, &[], None, &bad_filter)),
        (
            "grayscale-palette",
            png_with_chunks(8, 0, 0, &[&[0, 0, 0]], None, &indexed),
        ),
        (
            "duplicate-palette",
            png_with_chunks(1, 3, 0, &[&[0, 0, 0], &[255, 255, 255]], None, &indexed),
        ),
        (
            "oversized-palette",
            png_with_chunks(1, 3, 0, &[&[0, 0, 0, 1, 1, 1, 2, 2, 2]], None, &indexed),
        ),
        (
            "invalid-truecolor-transparency",
            png_with_chunks(8, 6, 0, &[], Some(&[0, 0]), &rgba),
        ),
    ];
    for (name, bytes) in cases {
        let path = temp.path().join(format!("{name}.png"));
        fs::write(&path, bytes).unwrap();
        assert!(matches!(
            ImageConverter.convert(&request(path)),
            Err(ConversionError::CorruptInput { .. })
        ));
    }

    let adam7 = temp.path().join("adam7.png");
    fs::write(&adam7, png_with_chunks(8, 6, 1, &[], None, &rgba)).unwrap();
    assert!(matches!(
        ImageConverter.convert(&request(adam7)),
        Err(ConversionError::UnsupportedInput { .. })
    ));
}

#[test]
fn png_requires_palette_before_transparency_when_both_are_present() {
    let temp = TempDir::new().unwrap();
    let invalid = temp.path().join("palette-after-transparency.png");
    fs::write(&invalid, truecolor_png_with_palette_and_transparency(false)).unwrap();
    assert!(matches!(
        ImageConverter.convert(&request(invalid)),
        Err(ConversionError::CorruptInput { .. })
    ));

    let valid = temp.path().join("palette-before-transparency.png");
    fs::write(&valid, truecolor_png_with_palette_and_transparency(true)).unwrap();
    assert!(ImageConverter.convert(&request(valid)).is_ok());
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
