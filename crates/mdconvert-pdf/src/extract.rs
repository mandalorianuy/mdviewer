use std::{collections::BTreeMap, fs, sync::Mutex};

use mdconvert_core::{ConversionError, ConversionRequest, DocumentMetadata};
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

pub fn extract_pdf(request: &ConversionRequest) -> Result<RawDocument, ConversionError> {
    let _extraction_guard =
        PDFIUM_EXTRACTION_LOCK
            .lock()
            .map_err(|_| ConversionError::ConversionFailed {
                message: "PDFium extraction state is unavailable".into(),
            })?;
    let bytes = read_source(request)?;
    let pdfium = load_pdfium()?;
    let document = pdfium
        .load_pdf_from_byte_vec(bytes, None)
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

    let metadata = extract_metadata(&document, page_count);
    let mut pages = Vec::with_capacity(page_count as usize);
    let mut contains_text = false;
    for (index, page) in document.pages().iter().enumerate() {
        let raw_page = extract_page(index, &page)?;
        contains_text |= raw_page.glyphs.iter().any(|glyph| {
            glyph
                .text
                .chars()
                .any(|character| !character.is_whitespace())
        });
        pages.push(raw_page);
    }
    if !contains_text {
        return Err(ConversionError::OcrRequired);
    }

    Ok(RawDocument { metadata, pages })
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
    let reported_width = finite_value("page width", page.width().value)?;
    let reported_height = finite_value("page height", page.height().value)?;
    let rotation = page
        .rotation()
        .map_err(|error| pdfium_error("read page rotation", error))?;
    let geometry = PageGeometry::new(reported_width, reported_height, rotation);
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
    source_width: f32,
    source_height: f32,
    display_width: f32,
    display_height: f32,
    rotation: PdfPageRenderRotation,
}

impl PageGeometry {
    fn new(reported_width: f32, reported_height: f32, rotation: PdfPageRenderRotation) -> Self {
        // PDFium reports the displayed page size, including intrinsic quarter-turns,
        // while page object coordinates remain in the unrotated page coordinate system.
        let (source_width, source_height) = match rotation {
            PdfPageRenderRotation::None | PdfPageRenderRotation::Degrees180 => {
                (reported_width, reported_height)
            }
            PdfPageRenderRotation::Degrees90 | PdfPageRenderRotation::Degrees270 => {
                (reported_height, reported_width)
            }
        };
        Self {
            source_width,
            source_height,
            display_width: reported_width,
            display_height: reported_height,
            rotation,
        }
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
        rect.left().value,
        geometry.source_height - rect.top().value,
        rect.right().value,
        geometry.source_height - rect.bottom().value,
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
