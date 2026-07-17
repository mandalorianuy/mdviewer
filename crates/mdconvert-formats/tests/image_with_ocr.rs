use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

use mdconvert_core::{Block, ConversionRequest, Inline, WarningCode};
use mdconvert_formats::ImageConverter;
use mdconvert_ocr::{OcrEngine, OcrError, OcrInput, OcrLine, OcrOutput, OcrRect};

struct ScriptedOcr {
    output: OcrOutput,
    calls: AtomicUsize,
}

impl ScriptedOcr {
    fn new(output: OcrOutput) -> Self {
        Self {
            output,
            calls: AtomicUsize::new(0),
        }
    }
}

impl OcrEngine for ScriptedOcr {
    fn name(&self) -> &'static str {
        "scripted"
    }

    fn recognize(&self, _input: OcrInput<'_>) -> Result<OcrOutput, OcrError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.output.clone())
    }
}

fn request() -> ConversionRequest {
    ConversionRequest::new(PathBuf::from("pixel.png")).unwrap()
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

fn chunk(output: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    output.extend_from_slice(&(data.len() as u32).to_be_bytes());
    output.extend_from_slice(kind);
    output.extend_from_slice(data);
    let mut checked = kind.to_vec();
    checked.extend_from_slice(data);
    output.extend_from_slice(&crc32(&checked).to_be_bytes());
}

fn pixel_png() -> Vec<u8> {
    let mut output = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&1_u32.to_be_bytes());
    ihdr.extend_from_slice(&1_u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut output, b"IHDR", &ihdr);
    chunk(
        &mut output,
        b"IDAT",
        &[0x78, 0x9c, 0x63, 0x60, 0, 2, 0, 0, 5, 0, 1],
    );
    chunk(&mut output, b"IEND", &[]);
    output
}

fn line(text: &str, confidence: f32, top: f32) -> OcrLine {
    OcrLine::new(text, confidence, OcrRect::new(0.1, top, 0.8, 0.1).unwrap())
}

#[test]
fn recognized_lines_become_ordered_paragraphs_before_the_original_asset() {
    let engine = ScriptedOcr::new(OcrOutput::new(vec![
        line("segunda", 0.9, 0.4),
        line("primera", 0.95, 0.1),
    ]));

    let document = ImageConverter
        .convert_owned_bytes_with_ocr(pixel_png(), &request(), &engine)
        .unwrap();

    assert_eq!(engine.calls.load(Ordering::SeqCst), 1);
    assert_eq!(document.metadata.properties["ocr_engine"], "scripted");
    assert!(matches!(
        &document.blocks[..],
        [
            Block::Paragraph { content: first },
            Block::Paragraph { content: second },
            Block::Image { .. }
        ] if first == &[Inline::Text("primera".into())]
            && second == &[Inline::Text("segunda".into())]
    ));
    assert!(document.warnings.is_empty());
}

#[test]
fn no_text_and_low_confidence_are_explicit_without_dropping_the_asset_or_text() {
    let empty = ScriptedOcr::new(OcrOutput::default());
    let document = ImageConverter
        .convert_owned_bytes_with_ocr(pixel_png(), &request(), &empty)
        .unwrap();
    assert_eq!(document.assets.len(), 1);
    assert_eq!(document.warnings[0].code, WarningCode::OcrNoTextFound);

    let low = ScriptedOcr::new(OcrOutput::new(vec![line("incierto", 0.49, 0.1)]));
    let document = ImageConverter
        .convert_owned_bytes_with_ocr(pixel_png(), &request(), &low)
        .unwrap();
    assert!(matches!(
        &document.blocks[0],
        Block::Paragraph { content } if content == &[Inline::Text("incierto".into())]
    ));
    assert_eq!(document.warnings[0].code, WarningCode::OcrLowConfidence);
}
