use mdconvert_ocr::{OcrEngine, OcrError, OcrInput, OcrLine, OcrOutput, OcrRect, OcrSource};

struct ScriptedEngine;

impl OcrEngine for ScriptedEngine {
    fn name(&self) -> &'static str {
        "scripted"
    }

    fn recognize(&self, input: OcrInput<'_>) -> Result<OcrOutput, OcrError> {
        assert_eq!(input.source(), OcrSource::Image);
        Ok(OcrOutput::new(vec![OcrLine::new(
            "Texto local",
            0.875,
            OcrRect::new(0.1, 0.2, 0.8, 0.1).unwrap(),
        )]))
    }
}

#[test]
fn contract_is_object_safe_send_sync_and_preserves_recognition_data() {
    fn require_send_sync<T: Send + Sync + ?Sized>() {}
    require_send_sync::<dyn OcrEngine>();

    let engine: &dyn OcrEngine = &ScriptedEngine;
    let input = OcrInput::new(b"encoded", "image/png", 800, 600, OcrSource::Image).unwrap();
    let output = engine.recognize(input).unwrap();

    assert_eq!(engine.name(), "scripted");
    assert_eq!(output.lines()[0].text(), "Texto local");
    assert_eq!(output.lines()[0].confidence(), 0.875);
    assert_eq!(output.lines()[0].bounds().top(), 0.2);
}

#[test]
fn input_rejects_empty_bytes_unknown_media_types_and_empty_dimensions() {
    for error in [
        OcrInput::new(b"", "image/png", 1, 1, OcrSource::Image).unwrap_err(),
        OcrInput::new(b"x", "image/gif", 1, 1, OcrSource::Image).unwrap_err(),
        OcrInput::new(b"x", "image/jpeg", 0, 1, OcrSource::PdfPage).unwrap_err(),
        OcrInput::new(b"x", "image/png", 1, 0, OcrSource::PdfPage).unwrap_err(),
    ] {
        assert_eq!(error.code(), "invalid_ocr_input");
    }
}

#[test]
fn normalized_rect_and_confidence_reject_non_finite_or_out_of_range_values() {
    for values in [
        (-0.1, 0.0, 0.5, 0.5),
        (0.0, 0.0, 1.1, 0.5),
        (0.8, 0.0, 0.3, 0.5),
        (0.0, 0.9, 0.5, 0.2),
        (f32::NAN, 0.0, 0.5, 0.5),
    ] {
        assert!(OcrRect::new(values.0, values.1, values.2, values.3).is_err());
    }

    let bounds = OcrRect::new(0.0, 0.0, 1.0, 1.0).unwrap();
    assert!(OcrLine::try_new("text", -0.01, bounds).is_err());
    assert!(OcrLine::try_new("text", 1.01, bounds).is_err());
    assert!(OcrLine::try_new("text", f32::NAN, bounds).is_err());
    assert!(OcrLine::try_new("   ", 0.5, bounds).is_err());
}

#[test]
fn errors_have_stable_redacted_codes() {
    assert_eq!(OcrError::Unavailable.code(), "ocr_unavailable");
    assert_eq!(OcrError::InvalidInput.code(), "invalid_ocr_input");
    assert_eq!(OcrError::RecognitionFailed.code(), "ocr_failed");
    assert!(!OcrError::RecognitionFailed.to_string().contains("document"));
}
