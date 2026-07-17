#![cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]

use font8x8::{BASIC_FONTS, UnicodeFonts};
use mdconvert_ocr::{LocalOcrEngine, OcrEngine, OcrInput, OcrSource};

fn text_png(text: &str) -> Vec<u8> {
    const SCALE: usize = 12;
    const MARGIN: usize = 24;
    let width = MARGIN * 2 + text.chars().count() * 8 * SCALE;
    let height = MARGIN * 2 + 8 * SCALE;
    let mut pixels = vec![255_u8; width * height];

    for (character_index, character) in text.chars().enumerate() {
        let glyph = BASIC_FONTS.get(character).expect("test glyph");
        for (row, bits) in glyph.into_iter().enumerate() {
            for column in 0..8 {
                if bits & (1 << column) == 0 {
                    continue;
                }
                let origin_x = MARGIN + (character_index * 8 + column) * SCALE;
                let origin_y = MARGIN + row * SCALE;
                for y in origin_y..origin_y + SCALE {
                    for x in origin_x..origin_x + SCALE {
                        pixels[y * width + x] = 0;
                    }
                }
            }
        }
    }

    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut encoded, width as u32, height as u32);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&pixels)
            .unwrap();
    }
    encoded
}

#[test]
fn local_backend_recognizes_a_stable_png_and_returns_normalized_bounds() {
    let bytes = text_png("HELLO 123");
    let input = OcrInput::new(&bytes, "image/png", 912, 144, OcrSource::Image).unwrap();
    let output = LocalOcrEngine.recognize(input).unwrap();
    let text = output
        .lines()
        .iter()
        .map(|line| line.text())
        .collect::<Vec<_>>()
        .join(" ");

    let recognized = text
        .split_whitespace()
        .map(|token| token.to_ascii_uppercase())
        .collect::<Vec<_>>();
    let expected = ["HELLO", "123"];
    let recovered = expected
        .iter()
        .filter(|token| recognized.iter().any(|actual| actual == **token))
        .count();
    let token_recall = recovered as f32 / expected.len() as f32;
    assert!(token_recall >= 0.95, "recall={token_recall}: {text:?}");
    assert!(output.lines().iter().all(|line| {
        let bounds = line.bounds();
        bounds.left() >= 0.0
            && bounds.top() >= 0.0
            && bounds.left() + bounds.width() <= 1.0
            && bounds.top() + bounds.height() <= 1.0
    }));
}
