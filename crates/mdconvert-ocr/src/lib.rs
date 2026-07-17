use thiserror::Error;

#[cfg(target_os = "linux")]
mod tesseract;
#[cfg(target_os = "macos")]
mod vision;
#[cfg(target_os = "windows")]
mod windows_ocr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcrSource {
    Image,
    PdfPage,
    PdfEmbeddedImage,
}

#[derive(Debug, Clone, Copy)]
pub struct OcrInput<'a> {
    bytes: &'a [u8],
    media_type: &'a str,
    width: u32,
    height: u32,
    source: OcrSource,
}

impl<'a> OcrInput<'a> {
    pub fn new(
        bytes: &'a [u8],
        media_type: &'a str,
        width: u32,
        height: u32,
        source: OcrSource,
    ) -> Result<Self, OcrError> {
        if bytes.is_empty()
            || !matches!(media_type, "image/png" | "image/jpeg")
            || width == 0
            || height == 0
        {
            return Err(OcrError::InvalidInput);
        }
        Ok(Self {
            bytes,
            media_type,
            width,
            height,
            source,
        })
    }

    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    pub fn media_type(&self) -> &'a str {
        self.media_type
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn source(&self) -> OcrSource {
        self.source
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OcrRect {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
}

impl OcrRect {
    pub fn new(left: f32, top: f32, width: f32, height: f32) -> Result<Self, OcrError> {
        if ![left, top, width, height].into_iter().all(f32::is_finite)
            || left < 0.0
            || top < 0.0
            || width <= 0.0
            || height <= 0.0
            || left + width > 1.0
            || top + height > 1.0
        {
            return Err(OcrError::InvalidInput);
        }
        Ok(Self {
            left,
            top,
            width,
            height,
        })
    }

    pub fn left(&self) -> f32 {
        self.left
    }

    pub fn top(&self) -> f32 {
        self.top
    }

    pub fn width(&self) -> f32 {
        self.width
    }

    pub fn height(&self) -> f32 {
        self.height
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcrLine {
    text: String,
    confidence: f32,
    bounds: OcrRect,
}

impl OcrLine {
    pub fn new(text: impl Into<String>, confidence: f32, bounds: OcrRect) -> Self {
        Self::try_new(text, confidence, bounds).expect("OCR line must be valid")
    }

    pub fn try_new(
        text: impl Into<String>,
        confidence: f32,
        bounds: OcrRect,
    ) -> Result<Self, OcrError> {
        let text = text.into();
        if text.trim().is_empty() || !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
            return Err(OcrError::InvalidInput);
        }
        Ok(Self {
            text,
            confidence,
            bounds,
        })
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn confidence(&self) -> f32 {
        self.confidence
    }

    pub fn bounds(&self) -> OcrRect {
        self.bounds
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct OcrOutput {
    lines: Vec<OcrLine>,
}

impl OcrOutput {
    pub fn new(mut lines: Vec<OcrLine>) -> Self {
        lines.sort_by(|left, right| {
            left.bounds
                .top
                .total_cmp(&right.bounds.top)
                .then_with(|| left.bounds.left.total_cmp(&right.bounds.left))
        });
        Self { lines }
    }

    pub fn lines(&self) -> &[OcrLine] {
        &self.lines
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

pub trait OcrEngine: Send + Sync {
    fn name(&self) -> &'static str;

    fn recognize(&self, input: OcrInput<'_>) -> Result<OcrOutput, OcrError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct LocalOcrEngine;

impl OcrEngine for LocalOcrEngine {
    fn name(&self) -> &'static str {
        if cfg!(target_os = "macos") {
            "apple_vision"
        } else if cfg!(target_os = "windows") {
            "windows_media_ocr"
        } else if cfg!(target_os = "linux") {
            "tesseract_5"
        } else {
            "unavailable"
        }
    }

    fn recognize(&self, input: OcrInput<'_>) -> Result<OcrOutput, OcrError> {
        #[cfg(target_os = "macos")]
        {
            vision::recognize(input)
        }
        #[cfg(target_os = "windows")]
        {
            windows_ocr::recognize(input)
        }
        #[cfg(target_os = "linux")]
        {
            tesseract::recognize(input)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            let _ = input;
            Err(OcrError::Unavailable)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum OcrError {
    #[error("local OCR is unavailable")]
    Unavailable,
    #[error("OCR input is invalid")]
    InvalidInput,
    #[error("local OCR failed")]
    RecognitionFailed,
}

impl OcrError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unavailable => "ocr_unavailable",
            Self::InvalidInput => "invalid_ocr_input",
            Self::RecognitionFailed => "ocr_failed",
        }
    }
}
