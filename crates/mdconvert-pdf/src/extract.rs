use std::{collections::BTreeMap, fs};

use mdconvert_core::{ConversionError, ConversionRequest, DocumentMetadata};
use pdfium_render::prelude::*;
use url::Url;

use crate::{
    bindings::load_pdfium,
    raw::{RawDocument, RawGlyph, RawImage, RawLink, RawPage, RawRect, RawRule, RawWord, RuleKind},
};

pub fn extract_pdf(request: &ConversionRequest) -> Result<RawDocument, ConversionError> {
    let bytes = read_source(request)?;
    let pdfium = load_pdfium()?;
    let document = pdfium
        .load_pdf_from_byte_vec(bytes, None)
        .map_err(map_document_load_error)?;
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
    let width = finite_value("page width", page.width().value)?;
    let height = finite_value("page height", page.height().value)?;
    let rotation_degrees = page
        .rotation()
        .map_err(|error| pdfium_error("read page rotation", error))?
        .as_degrees() as i16;
    let glyphs = extract_glyphs(page, height)?;
    let words = group_words(&glyphs);
    let (images, rules) = extract_objects(page, height)?;
    let links = extract_links(page, height)?;

    Ok(RawPage {
        number: u32::try_from(index + 1).map_err(|_| ConversionError::ConversionFailed {
            message: "PDF page number cannot be represented as u32".into(),
        })?,
        width,
        height,
        rotation_degrees,
        glyphs,
        words,
        images,
        links,
        rules,
    })
}

fn extract_glyphs(page: &PdfPage<'_>, page_height: f32) -> Result<Vec<RawGlyph>, ConversionError> {
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
            bounds: rect_from_pdf(bounds, page_height)?,
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
    page_height: f32,
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
                    page_height,
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
                    page_height,
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
    if segments.len() >= 4 {
        let collected = segments.iter().collect::<Vec<_>>();
        let uses_only_lines = collected.first()?.segment_type() == PdfPathSegmentType::MoveTo
            && collected
                .iter()
                .skip(1)
                .all(|segment| segment.segment_type() == PdfPathSegmentType::LineTo);
        if uses_only_lines && collected.last()?.is_close() {
            return Some(RuleKind::Rectangle);
        }
    }
    None
}

fn extract_links(page: &PdfPage<'_>, page_height: f32) -> Result<Vec<RawLink>, ConversionError> {
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
        let Ok(bounds) = rect_from_pdf(bounds, page_height) else {
            continue;
        };
        links.push(RawLink { bounds, target });
    }
    Ok(links)
}

fn rect_from_pdf(rect: PdfRect, page_height: f32) -> Result<RawRect, ConversionError> {
    RawRect::try_new(
        rect.left().value,
        page_height - rect.top().value,
        rect.right().value,
        page_height - rect.bottom().value,
    )
    .ok_or_else(|| ConversionError::ConversionFailed {
        message: "PDFium returned non-finite rectangle coordinates".into(),
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
