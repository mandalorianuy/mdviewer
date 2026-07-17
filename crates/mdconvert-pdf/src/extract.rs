use std::{collections::BTreeMap, fs, sync::Mutex};

use mdconvert_core::{Cancellation, ConversionError, ConversionRequest, DocumentMetadata};
use mdconvert_ocr::{OcrEngine, OcrError, OcrInput, OcrSource};
use pdfium_render::prelude::*;
use url::Url;

use crate::{
    bindings::load_pdfium,
    raw::{RawDocument, RawGlyph, RawImage, RawLink, RawPage, RawRect, RawRule, RawWord, RuleKind},
};

// The pinned native PDFium build aborts when independent documents are loaded and
// destroyed concurrently, even through pdfium-render's thread-safe bindings. Keep
// each extraction transaction serialized at the native-library boundary.
static PDFIUM_EXTRACTION_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PdfOcrLimits {
    pub dpi: u16,
    pub max_dimension: u32,
    pub max_pixels_per_page: u64,
    pub max_total_pixels: u64,
}

impl Default for PdfOcrLimits {
    fn default() -> Self {
        Self {
            dpi: 200,
            max_dimension: 4_096,
            max_pixels_per_page: 16_000_000,
            max_total_pixels: 64_000_000,
        }
    }
}

impl PdfOcrLimits {
    fn validate(self) -> Result<Self, ConversionError> {
        if self.dpi == 0
            || self.max_dimension == 0
            || self.max_pixels_per_page == 0
            || self.max_total_pixels == 0
        {
            return Err(ConversionError::ConversionFailed {
                message: "PDF OCR limits must be positive and internally consistent".into(),
            });
        }
        Ok(self)
    }
}

pub fn extract_pdf(request: &ConversionRequest) -> Result<RawDocument, ConversionError> {
    let bytes = read_source(request)?;
    extract_pdf_bytes(&bytes, request)
}

pub fn extract_pdf_with_ocr(
    request: &ConversionRequest,
    engine: &dyn OcrEngine,
) -> Result<RawDocument, ConversionError> {
    let bytes = read_source(request)?;
    extract_pdf_bytes_with_ocr(&bytes, request, engine)
}

pub fn extract_pdf_bytes(
    bytes: &[u8],
    request: &ConversionRequest,
) -> Result<RawDocument, ConversionError> {
    extract_pdf_bytes_inner(bytes, request, None)
}

pub fn extract_pdf_bytes_with_ocr(
    bytes: &[u8],
    request: &ConversionRequest,
    engine: &dyn OcrEngine,
) -> Result<RawDocument, ConversionError> {
    extract_pdf_bytes_inner(bytes, request, Some(engine))
}

fn extract_pdf_bytes_inner(
    bytes: &[u8],
    request: &ConversionRequest,
    ocr: Option<&dyn OcrEngine>,
) -> Result<RawDocument, ConversionError> {
    extract_pdf_bytes_inner_with_limits(bytes, request, ocr, PdfOcrLimits::default(), None)
}

pub fn extract_pdf_bytes_with_ocr_limits(
    bytes: &[u8],
    request: &ConversionRequest,
    engine: &dyn OcrEngine,
    limits: PdfOcrLimits,
) -> Result<RawDocument, ConversionError> {
    extract_pdf_bytes_inner_with_limits(bytes, request, Some(engine), limits, None)
}

pub fn extract_pdf_bytes_with_ocr_cancellable(
    bytes: &[u8],
    request: &ConversionRequest,
    engine: &dyn OcrEngine,
    cancellation: &dyn Cancellation,
) -> Result<RawDocument, ConversionError> {
    extract_pdf_bytes_inner_with_limits(
        bytes,
        request,
        Some(engine),
        PdfOcrLimits::default(),
        Some(cancellation),
    )
}

fn extract_pdf_bytes_inner_with_limits(
    bytes: &[u8],
    request: &ConversionRequest,
    ocr: Option<&dyn OcrEngine>,
    ocr_limits: PdfOcrLimits,
    cancellation: Option<&dyn Cancellation>,
) -> Result<RawDocument, ConversionError> {
    let ocr_limits = ocr_limits.validate()?;
    check_cancellation(cancellation)?;
    let actual = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if actual > request.limits.max_input_bytes {
        return Err(ConversionError::LimitExceeded {
            limit: "input_bytes",
            actual,
            maximum: request.limits.max_input_bytes,
        });
    }
    let _extraction_guard =
        PDFIUM_EXTRACTION_LOCK
            .lock()
            .map_err(|_| ConversionError::ConversionFailed {
                message: "PDFium extraction state is unavailable".into(),
            })?;
    let pdfium = load_pdfium()?;
    let document = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .map_err(map_document_load_error)?;
    ensure_unencrypted(&document)?;
    let page_count =
        u32::try_from(document.pages().len()).map_err(|_| ConversionError::ConversionFailed {
            message: "PDF page count cannot be represented as u32".into(),
        })?;
    if page_count > request.limits.max_pages {
        return Err(ConversionError::LimitExceeded {
            limit: "max_pages",
            actual: u64::from(page_count),
            maximum: u64::from(request.limits.max_pages),
        });
    }
    preflight_images(&document, request)?;

    let mut metadata = extract_metadata(&document, page_count);
    let mut pages = Vec::with_capacity(page_count as usize);
    let mut contains_text = false;
    let mut rendered_ocr_pixels = 0_u64;
    let mut ocr_pages = Vec::new();
    let mut ocr_deferred_pages = Vec::new();
    let mut ocr_no_text_pages = Vec::new();
    let mut ocr_low_confidence_pages = Vec::new();
    for (index, page) in document.pages().iter().enumerate() {
        check_cancellation(cancellation)?;
        let mut raw_page = extract_page(index, &page)?;
        let had_digital_text = page_contains_text(&raw_page);
        if !had_digital_text && let Some(engine) = ocr {
            let outcome = apply_page_ocr(
                &page,
                &mut raw_page,
                engine,
                &mut rendered_ocr_pixels,
                ocr_limits,
            )?;
            check_cancellation(cancellation)?;
            ocr_pages.push(raw_page.number);
            if outcome.unavailable {
                ocr_deferred_pages.push(raw_page.number);
            }
            if !outcome.had_text {
                ocr_no_text_pages.push(raw_page.number);
            }
            if outcome.low_confidence {
                ocr_low_confidence_pages.push(raw_page.number);
            }
        }
        contains_text |= page_contains_text(&raw_page);
        pages.push(raw_page);
    }
    if !contains_text {
        return Err(ConversionError::OcrRequired);
    }

    if let Some(engine) = ocr
        && !ocr_pages.is_empty()
    {
        metadata
            .properties
            .insert("ocr_engine".into(), engine.name().into());
        metadata
            .properties
            .insert("ocr_pages".into(), page_list(&ocr_pages));
        if !ocr_deferred_pages.is_empty() {
            metadata
                .properties
                .insert("ocr_deferred_pages".into(), page_list(&ocr_deferred_pages));
        }
        if !ocr_no_text_pages.is_empty() {
            metadata
                .properties
                .insert("ocr_no_text_pages".into(), page_list(&ocr_no_text_pages));
        }
        if !ocr_low_confidence_pages.is_empty() {
            metadata.properties.insert(
                "ocr_low_confidence_pages".into(),
                page_list(&ocr_low_confidence_pages),
            );
        }
    }

    Ok(RawDocument { metadata, pages })
}

fn check_cancellation(cancellation: Option<&dyn Cancellation>) -> Result<(), ConversionError> {
    if cancellation.is_some_and(Cancellation::is_cancelled) {
        Err(ConversionError::Cancelled)
    } else {
        Ok(())
    }
}

fn page_contains_text(page: &RawPage) -> bool {
    page.glyphs.iter().any(|glyph| {
        glyph
            .text
            .chars()
            .any(|character| !character.is_whitespace())
    })
}

fn page_list(pages: &[u32]) -> String {
    pages
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

#[derive(Debug, Clone, Copy)]
struct PageOcrOutcome {
    had_text: bool,
    low_confidence: bool,
    unavailable: bool,
}

fn apply_page_ocr(
    page: &PdfPage<'_>,
    raw_page: &mut RawPage,
    engine: &dyn OcrEngine,
    total_pixels: &mut u64,
    limits: PdfOcrLimits,
) -> Result<PageOcrOutcome, ConversionError> {
    const POINTS_PER_INCH: f64 = 72.0;

    let mut width = f64::from(raw_page.width) * f64::from(limits.dpi) / POINTS_PER_INCH;
    let mut height = f64::from(raw_page.height) * f64::from(limits.dpi) / POINTS_PER_INCH;
    let max_dimension = f64::from(limits.max_dimension);
    let dimension_scale = (max_dimension / width).min(max_dimension / height).min(1.0);
    width *= dimension_scale;
    height *= dimension_scale;
    let page_pixels = (width.ceil() as u64).saturating_mul(height.ceil() as u64);
    if page_pixels > limits.max_pixels_per_page {
        let pixel_scale = (limits.max_pixels_per_page as f64 / page_pixels as f64).sqrt();
        width *= pixel_scale;
        height *= pixel_scale;
    }
    let width = width.floor().max(1.0) as u32;
    let height = height.floor().max(1.0) as u32;
    let page_pixels = u64::from(width).saturating_mul(u64::from(height));
    if page_pixels > limits.max_pixels_per_page {
        return Err(ConversionError::LimitExceeded {
            limit: "pdf_ocr_page_pixels",
            actual: page_pixels,
            maximum: limits.max_pixels_per_page,
        });
    }
    let next_total = total_pixels.saturating_add(page_pixels);
    if next_total > limits.max_total_pixels {
        return Err(ConversionError::LimitExceeded {
            limit: "pdf_ocr_rendered_pixels",
            actual: next_total,
            maximum: limits.max_total_pixels,
        });
    }

    let bitmap = page
        .render_with_config(
            &PdfRenderConfig::new()
                .set_fixed_size(width as i32, height as i32)
                .render_annotations(false)
                .render_form_data(false),
        )
        .map_err(|error| pdfium_error("render PDF page for local OCR", error))?;
    *total_pixels = next_total;
    let png = encode_rendered_page(&bitmap, width, height)?;
    let input = OcrInput::new(&png, "image/png", width, height, OcrSource::PdfPage)
        .map_err(map_ocr_error)?;
    let output = match engine.recognize(input) {
        Ok(output) => output,
        Err(OcrError::Unavailable) => {
            return Ok(PageOcrOutcome {
                had_text: false,
                low_confidence: false,
                unavailable: true,
            });
        }
        Err(error) => return Err(map_ocr_error(error)),
    };
    let low_confidence = output.lines().iter().any(|line| line.confidence() < 0.5);
    for line in output.lines() {
        let bounds = line.bounds();
        let rect = RawRect::try_new(
            bounds.left() * raw_page.width,
            bounds.top() * raw_page.height,
            (bounds.left() + bounds.width()) * raw_page.width,
            (bounds.top() + bounds.height()) * raw_page.height,
        )
        .ok_or_else(|| ConversionError::ConversionFailed {
            message: "local OCR returned invalid page geometry".into(),
        })?;
        let glyph_start = raw_page.glyphs.len();
        raw_page.glyphs.push(RawGlyph {
            text: line.text().into(),
            bounds: rect,
            font_size: rect.height().max(1.0),
            font_name: Some("Local OCR".into()),
            font_weight: None,
        });
        raw_page.words.push(RawWord {
            text: line.text().into(),
            bounds: rect,
            glyph_start,
            glyph_end: glyph_start + 1,
        });
    }
    Ok(PageOcrOutcome {
        had_text: !output.is_empty(),
        low_confidence,
        unavailable: false,
    })
}

fn encode_rendered_page(
    bitmap: &PdfBitmap<'_>,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, ConversionError> {
    let rgba = bitmap.as_rgba_bytes();
    let expected = u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(4);
    if u64::try_from(rgba.len()).unwrap_or(u64::MAX) != expected {
        return Err(ConversionError::ConversionFailed {
            message: "PDFium OCR render returned an unexpected pixel buffer".into(),
        });
    }
    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut encoded, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .and_then(|mut writer| writer.write_image_data(&rgba))
            .map_err(|_| ConversionError::ConversionFailed {
                message: "could not encode a bounded PDF page for local OCR".into(),
            })?;
    }
    Ok(encoded)
}

fn map_ocr_error(error: OcrError) -> ConversionError {
    match error {
        OcrError::Unavailable => ConversionError::OcrRequired,
        error => ConversionError::ConversionFailed {
            message: format!("local PDF OCR failed with {}", error.code()),
        },
    }
}

fn read_source(request: &ConversionRequest) -> Result<Vec<u8>, ConversionError> {
    let metadata = fs::metadata(&request.source).map_err(|source| ConversionError::Io {
        path: request.source.clone(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(ConversionError::CorruptInput {
            message: format!(
                "PDF source is not a regular file: {}",
                request.source.display()
            ),
        });
    }
    if metadata.len() > request.limits.max_input_bytes {
        return Err(ConversionError::LimitExceeded {
            limit: "input_bytes",
            actual: metadata.len(),
            maximum: request.limits.max_input_bytes,
        });
    }

    let bytes = fs::read(&request.source).map_err(|source| ConversionError::Io {
        path: request.source.clone(),
        source,
    })?;
    let actual = bytes.len() as u64;
    if actual > request.limits.max_input_bytes {
        return Err(ConversionError::LimitExceeded {
            limit: "input_bytes",
            actual,
            maximum: request.limits.max_input_bytes,
        });
    }
    Ok(bytes)
}

fn map_document_load_error(error: PdfiumError) -> ConversionError {
    match error {
        PdfiumError::PdfiumLibraryInternalError(
            PdfiumInternalError::PasswordError | PdfiumInternalError::SecurityError,
        ) => ConversionError::EncryptedInput,
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::FormatError) => {
            ConversionError::CorruptInput {
                message: "PDFium rejected the PDF structure".into(),
            }
        }
        error => pdfium_error("load PDF document", error),
    }
}

fn ensure_unencrypted(document: &PdfDocument<'_>) -> Result<(), ConversionError> {
    match document.permissions().security_handler_revision() {
        Ok(PdfSecurityHandlerRevision::Unprotected) => Ok(()),
        Ok(
            PdfSecurityHandlerRevision::Revision2
            | PdfSecurityHandlerRevision::Revision3
            | PdfSecurityHandlerRevision::Revision4,
        )
        | Err(PdfiumError::UnknownPdfSecurityHandlerRevision) => {
            Err(ConversionError::EncryptedInput)
        }
        Err(error) => Err(pdfium_error("inspect PDF security handler", error)),
    }
}

fn preflight_images(
    document: &PdfDocument<'_>,
    request: &ConversionRequest,
) -> Result<(), ConversionError> {
    let mut image_count = 0_u64;
    for page in document.pages().iter() {
        for object in page.objects().iter() {
            if object.as_image_object().is_some() {
                image_count = image_count.saturating_add(1);
            }
        }
    }
    if image_count > u64::from(request.limits.max_assets) {
        return Err(ConversionError::LimitExceeded {
            limit: "max_assets",
            actual: image_count,
            maximum: u64::from(request.limits.max_assets),
        });
    }

    // The input-byte ceiling is also the extraction memory ceiling. Account for the
    // fully decoded RGBA representation of every image before decoding any image.
    let mut decoded_bytes = 0_u64;
    for page in document.pages().iter() {
        for object in page.objects().iter() {
            let Some(image) = object.as_image_object() else {
                continue;
            };
            let width = u64::try_from(
                image
                    .width()
                    .map_err(|error| pdfium_error("read PDF image width", error))?,
            )
            .map_err(|_| ConversionError::ConversionFailed {
                message: "PDFium returned a negative image width".into(),
            })?;
            let height = u64::try_from(
                image
                    .height()
                    .map_err(|error| pdfium_error("read PDF image height", error))?,
            )
            .map_err(|_| ConversionError::ConversionFailed {
                message: "PDFium returned a negative image height".into(),
            })?;
            let image_bytes = width
                .checked_mul(height)
                .and_then(|pixels| pixels.checked_mul(4))
                .unwrap_or(u64::MAX);
            decoded_bytes = decoded_bytes.saturating_add(image_bytes);
        }
    }
    if decoded_bytes > request.limits.max_input_bytes {
        return Err(ConversionError::LimitExceeded {
            limit: "pdf_decoded_image_bytes",
            actual: decoded_bytes,
            maximum: request.limits.max_input_bytes,
        });
    }

    Ok(())
}

fn extract_metadata(document: &PdfDocument<'_>, page_count: u32) -> DocumentMetadata {
    let mut title = None;
    let mut author = None;
    let mut subject = None;
    let mut properties = BTreeMap::new();
    for tag in document.metadata().iter() {
        let value = tag.value().to_owned();
        match tag.tag_type() {
            PdfDocumentMetadataTagType::Title => title = Some(value),
            PdfDocumentMetadataTagType::Author => author = Some(value),
            PdfDocumentMetadataTagType::Subject => subject = Some(value),
            PdfDocumentMetadataTagType::Keywords => {
                properties.insert("keywords".into(), value);
            }
            PdfDocumentMetadataTagType::Creator => {
                properties.insert("creator".into(), value);
            }
            PdfDocumentMetadataTagType::Producer => {
                properties.insert("producer".into(), value);
            }
            PdfDocumentMetadataTagType::CreationDate => {
                properties.insert("creation_date".into(), value);
            }
            PdfDocumentMetadataTagType::ModificationDate => {
                properties.insert("modification_date".into(), value);
            }
        }
    }
    // pdfium-render 0.9.3 requests the non-standard `ModificationDate` metadata
    // key, so it cannot safely expose the PDF-standard `ModDate` value. Make the
    // unsupported state explicit instead of silently dropping the field.
    if !properties.contains_key("modification_date") {
        properties.insert(
            "modification_date_status".into(),
            "unsupported_by_pdfium_render_0_9_3".into(),
        );
    }

    DocumentMetadata {
        title,
        author,
        subject,
        source_format: Some("pdf".into()),
        page_count: Some(page_count),
        properties,
    }
}

fn extract_page(index: usize, page: &PdfPage<'_>) -> Result<RawPage, ConversionError> {
    let rotation = page
        .rotation()
        .map_err(|error| pdfium_error("read page rotation", error))?;
    let boundary = page
        .boundaries()
        .bounding()
        .map_err(|error| pdfium_error("read effective page boundary", error))?;
    let geometry = PageGeometry::new(boundary.bounds, rotation)?;
    let glyphs = extract_glyphs(page, geometry)?;
    let words = group_words(&glyphs);
    let (images, rules) = extract_objects(page, geometry)?;
    let links = extract_links(page, geometry)?;

    Ok(RawPage {
        number: u32::try_from(index + 1).map_err(|_| ConversionError::ConversionFailed {
            message: "PDF page number cannot be represented as u32".into(),
        })?,
        width: geometry.display_width,
        height: geometry.display_height,
        rotation_degrees: geometry.rotation.as_degrees() as i16,
        glyphs,
        words,
        images,
        links,
        rules,
    })
}

#[derive(Clone, Copy)]
struct PageGeometry {
    source_left: f32,
    source_bottom: f32,
    source_width: f32,
    source_height: f32,
    display_width: f32,
    display_height: f32,
    rotation: PdfPageRenderRotation,
}

impl PageGeometry {
    fn new(boundary: PdfRect, rotation: PdfPageRenderRotation) -> Result<Self, ConversionError> {
        let source_left = finite_value("effective page boundary left", boundary.left().value)?;
        let source_bottom =
            finite_value("effective page boundary bottom", boundary.bottom().value)?;
        let source_right = finite_value("effective page boundary right", boundary.right().value)?;
        let source_top = finite_value("effective page boundary top", boundary.top().value)?;
        if source_right <= source_left || source_top <= source_bottom {
            return Err(ConversionError::ConversionFailed {
                message: "PDFium returned an empty or inverted effective page boundary".into(),
            });
        }
        let source_width =
            finite_value("effective page boundary width", source_right - source_left)?;
        let source_height =
            finite_value("effective page boundary height", source_top - source_bottom)?;
        let (display_width, display_height) = match rotation {
            PdfPageRenderRotation::None | PdfPageRenderRotation::Degrees180 => {
                (source_width, source_height)
            }
            PdfPageRenderRotation::Degrees90 | PdfPageRenderRotation::Degrees270 => {
                (source_height, source_width)
            }
        };
        Ok(Self {
            source_left,
            source_bottom,
            source_width,
            source_height,
            display_width,
            display_height,
            rotation,
        })
    }
}

fn extract_glyphs(
    page: &PdfPage<'_>,
    geometry: PageGeometry,
) -> Result<Vec<RawGlyph>, ConversionError> {
    let text = page
        .text()
        .map_err(|error| pdfium_error("load page text", error))?;
    let mut glyphs = Vec::with_capacity(text.chars().len());
    for character in text.chars().iter() {
        let Some(value) = character.unicode_string() else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        let bounds = character
            .tight_bounds()
            .map_err(|error| pdfium_error("read glyph bounds", error))?;
        let font_name = character.font_name();
        glyphs.push(RawGlyph {
            text: value,
            bounds: rect_from_pdf(bounds, geometry)?,
            font_size: finite_value("glyph font size", character.scaled_font_size().value)?,
            font_name: (!font_name.is_empty()).then_some(font_name),
            font_weight: character.font_weight().and_then(font_weight_value),
        });
    }
    Ok(glyphs)
}

fn font_weight_value(weight: PdfFontWeight) -> Option<u16> {
    Some(match weight {
        PdfFontWeight::Weight100 => 100,
        PdfFontWeight::Weight200 => 200,
        PdfFontWeight::Weight300 => 300,
        PdfFontWeight::Weight400Normal => 400,
        PdfFontWeight::Weight500 => 500,
        PdfFontWeight::Weight600 => 600,
        PdfFontWeight::Weight700Bold => 700,
        PdfFontWeight::Weight800 => 800,
        PdfFontWeight::Weight900 => 900,
        PdfFontWeight::Custom(value) => value.try_into().ok()?,
    })
}

fn group_words(glyphs: &[RawGlyph]) -> Vec<RawWord> {
    let mut words = Vec::new();
    let mut start = None;
    for (index, glyph) in glyphs.iter().enumerate() {
        let is_whitespace = glyph.text.chars().all(char::is_whitespace);
        match (start, is_whitespace) {
            (None, false) => start = Some(index),
            (Some(word_start), true) => {
                words.push(build_word(glyphs, word_start, index));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(word_start) = start {
        words.push(build_word(glyphs, word_start, glyphs.len()));
    }
    words
}

fn build_word(glyphs: &[RawGlyph], start: usize, end: usize) -> RawWord {
    let mut bounds = glyphs[start].bounds;
    let mut text = String::new();
    for glyph in &glyphs[start..end] {
        text.push_str(&glyph.text);
        bounds = bounds.union(glyph.bounds);
    }
    RawWord {
        text,
        bounds,
        glyph_start: start,
        glyph_end: end,
    }
}

fn extract_objects(
    page: &PdfPage<'_>,
    geometry: PageGeometry,
) -> Result<(Vec<RawImage>, Vec<RawRule>), ConversionError> {
    let mut images = Vec::new();
    let mut rules = Vec::new();
    for object in page.objects().iter() {
        if let Some(image) = object.as_image_object() {
            let decoded = image
                .get_raw_image()
                .map_err(|error| pdfium_error("decode PDF image", error))?
                .to_rgba8();
            let (pixel_width, pixel_height) = decoded.dimensions();
            images.push(RawImage {
                index: u32::try_from(images.len() + 1).map_err(|_| {
                    ConversionError::ConversionFailed {
                        message: "PDF image index cannot be represented as u32".into(),
                    }
                })?,
                bounds: rect_from_pdf(
                    image
                        .bounds()
                        .map_err(|error| pdfium_error("read image bounds", error))?
                        .to_rect(),
                    geometry,
                )?,
                pixel_width,
                pixel_height,
                rgba: decoded.into_raw(),
            });
        }

        if let Some(path) = object.as_path_object()
            && path
                .is_stroked()
                .map_err(|error| pdfium_error("read path stroke mode", error))?
            && let Some(kind) = classify_rule(path)
        {
            rules.push(RawRule {
                kind,
                bounds: rect_from_pdf(
                    path.bounds()
                        .map_err(|error| pdfium_error("read rule bounds", error))?
                        .to_rect(),
                    geometry,
                )?,
                stroke_width: finite_value(
                    "path stroke width",
                    path.stroke_width()
                        .map_err(|error| pdfium_error("read path stroke width", error))?
                        .value,
                )?,
            });
        }
    }
    Ok((images, rules))
}

fn classify_rule(path: &PdfPagePathObject<'_>) -> Option<RuleKind> {
    let segments = path.segments();
    if segments.len() == 2 {
        let mut iter = segments.iter();
        return matches!(
            (iter.next()?.segment_type(), iter.next()?.segment_type()),
            (PdfPathSegmentType::MoveTo, PdfPathSegmentType::LineTo)
        )
        .then_some(RuleKind::Line);
    }
    if segments.len() == 5 {
        let matrix = path.matrix().ok()?;
        let transformed = segments.transform(matrix);
        let collected = transformed.iter().collect::<Vec<_>>();
        let uses_only_lines = collected.first()?.segment_type() == PdfPathSegmentType::MoveTo
            && collected
                .iter()
                .skip(1)
                .all(|segment| segment.segment_type() == PdfPathSegmentType::LineTo);
        if uses_only_lines && collected.last()?.is_close() && is_axis_aligned_rectangle(&collected)
        {
            return Some(RuleKind::Rectangle);
        }
    }
    None
}

const RECTANGLE_POINT_TOLERANCE: f32 = 0.01;

fn is_axis_aligned_rectangle(segments: &[PdfPathSegment<'_>]) -> bool {
    if segments.len() != 5 {
        return false;
    }
    let points = segments
        .iter()
        .map(|segment| {
            let (x, y) = segment.point();
            (x.value, y.value)
        })
        .collect::<Vec<_>>();
    if !points.iter().all(|(x, y)| x.is_finite() && y.is_finite())
        || !point_close(points[0], points[4])
    {
        return false;
    }

    let corners = &points[..4];
    for index in 0..4 {
        let start = corners[index];
        let end = corners[(index + 1) % 4];
        let same_x = close(start.0, end.0);
        let same_y = close(start.1, end.1);
        if same_x == same_y {
            return false;
        }
    }

    let min_x = corners
        .iter()
        .map(|point| point.0)
        .fold(f32::INFINITY, f32::min);
    let max_x = corners
        .iter()
        .map(|point| point.0)
        .fold(f32::NEG_INFINITY, f32::max);
    let min_y = corners
        .iter()
        .map(|point| point.1)
        .fold(f32::INFINITY, f32::min);
    let max_y = corners
        .iter()
        .map(|point| point.1)
        .fold(f32::NEG_INFINITY, f32::max);
    if close(min_x, max_x) || close(min_y, max_y) {
        return false;
    }
    let expected = [
        (min_x, min_y),
        (min_x, max_y),
        (max_x, min_y),
        (max_x, max_y),
    ];
    expected.iter().all(|expected| {
        corners
            .iter()
            .filter(|corner| point_close(**corner, *expected))
            .count()
            == 1
    })
}

fn close(left: f32, right: f32) -> bool {
    (left - right).abs() <= RECTANGLE_POINT_TOLERANCE
}

fn point_close(left: (f32, f32), right: (f32, f32)) -> bool {
    close(left.0, right.0) && close(left.1, right.1)
}

fn extract_links(
    page: &PdfPage<'_>,
    geometry: PageGeometry,
) -> Result<Vec<RawLink>, ConversionError> {
    let mut links = Vec::new();
    for link in page.links().iter() {
        let Some(PdfAction::Uri(action)) = link.action() else {
            continue;
        };
        let Ok(target) = action.uri() else {
            continue;
        };
        if Url::parse(&target).is_err() {
            continue;
        }
        let Ok(bounds) = link.rect() else {
            continue;
        };
        let Ok(bounds) = rect_from_pdf(bounds, geometry) else {
            continue;
        };
        links.push(RawLink { bounds, target });
    }
    Ok(links)
}

fn rect_from_pdf(rect: PdfRect, geometry: PageGeometry) -> Result<RawRect, ConversionError> {
    let unrotated = RawRect::try_new(
        rect.left().value - geometry.source_left,
        geometry.source_height - (rect.top().value - geometry.source_bottom),
        rect.right().value - geometry.source_left,
        geometry.source_height - (rect.bottom().value - geometry.source_bottom),
    )
    .ok_or_else(|| ConversionError::ConversionFailed {
        message: "PDFium returned non-finite rectangle coordinates".into(),
    })?;

    let rotated = match geometry.rotation {
        PdfPageRenderRotation::None => Some(unrotated),
        PdfPageRenderRotation::Degrees90 => RawRect::try_new(
            geometry.source_height - unrotated.bottom,
            unrotated.left,
            geometry.source_height - unrotated.top,
            unrotated.right,
        ),
        PdfPageRenderRotation::Degrees180 => RawRect::try_new(
            geometry.source_width - unrotated.right,
            geometry.source_height - unrotated.bottom,
            geometry.source_width - unrotated.left,
            geometry.source_height - unrotated.top,
        ),
        PdfPageRenderRotation::Degrees270 => RawRect::try_new(
            unrotated.top,
            geometry.source_width - unrotated.right,
            unrotated.bottom,
            geometry.source_width - unrotated.left,
        ),
    };
    rotated.ok_or_else(|| ConversionError::ConversionFailed {
        message: "PDFium returned non-finite rotated rectangle coordinates".into(),
    })
}

fn finite_value(label: &str, value: f32) -> Result<f32, ConversionError> {
    value
        .is_finite()
        .then_some(value)
        .ok_or_else(|| ConversionError::ConversionFailed {
            message: format!("PDFium returned a non-finite {label}"),
        })
}

fn pdfium_error(context: &str, error: PdfiumError) -> ConversionError {
    ConversionError::ConversionFailed {
        message: format!("could not {context}: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_and_security_load_errors_are_encrypted_input() {
        for internal in [
            PdfiumInternalError::PasswordError,
            PdfiumInternalError::SecurityError,
        ] {
            assert!(matches!(
                map_document_load_error(PdfiumError::PdfiumLibraryInternalError(internal)),
                ConversionError::EncryptedInput
            ));
        }
    }

    #[test]
    fn font_weights_outside_the_raw_contract_are_absent() {
        assert_eq!(font_weight_value(PdfFontWeight::Weight700Bold), Some(700));
        assert_eq!(font_weight_value(PdfFontWeight::Custom(u32::MAX)), None);
    }
}
