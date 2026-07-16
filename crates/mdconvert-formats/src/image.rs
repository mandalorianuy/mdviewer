use std::{collections::BTreeMap, io::Read};

use crc32fast::Hasher;
use flate2::read::ZlibDecoder;
use mdconvert_core::{
    Asset, AssetId, Block, ConversionError, ConversionRequest, ConversionWarning, Converter,
    Document, DocumentMetadata, WarningCode,
};

use crate::{limit_exceeded, read_input};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageLimits {
    pub max_chunks_or_segments: u64,
    pub max_metadata_bytes: u64,
    pub max_width: u64,
    pub max_height: u64,
    pub max_pixels: u64,
}

impl Default for ImageLimits {
    fn default() -> Self {
        Self {
            max_chunks_or_segments: 16_384,
            max_metadata_bytes: 1024 * 1024,
            max_width: 100_000,
            max_height: 100_000,
            max_pixels: 100_000_000,
        }
    }
}

impl ImageLimits {
    fn validate(&self) -> Result<(), ConversionError> {
        if self.max_chunks_or_segments == 0
            || self.max_metadata_bytes == 0
            || self.max_width == 0
            || self.max_height == 0
            || self.max_pixels == 0
        {
            return Err(ConversionError::ConversionFailed {
                message: "image limits must be greater than zero".into(),
            });
        }
        Ok(())
    }
}

pub(crate) fn validate_embedded_image(
    bytes: &[u8],
    media_type: &str,
) -> Result<&'static str, ConversionError> {
    let limits = ImageLimits::default();
    limits.validate()?;
    let parsed = match media_type {
        "image/png" => parse_png(bytes, &limits)?,
        "image/jpeg" => parse_jpeg(bytes, &limits)?,
        _ => {
            return Err(corrupt_error(format!(
                "unsupported embedded image media type {media_type:?}; local v1 accepts PNG/JPEG"
            )));
        }
    };
    if parsed.media_type != media_type {
        return Err(corrupt_error(
            "embedded image media type mismatches its bytes",
        ));
    }
    validate_dimensions(parsed.width, parsed.height, &limits)?;
    Ok(parsed.canonical_extension)
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ImageConverter;

impl ImageConverter {
    pub fn with_limits(limits: ImageLimits) -> BoundedImageConverter {
        BoundedImageConverter { limits }
    }
}

pub struct BoundedImageConverter {
    limits: ImageLimits,
}

impl Converter for ImageConverter {
    fn convert(&self, request: &ConversionRequest) -> Result<Document, ConversionError> {
        convert(request, &ImageLimits::default())
    }
}

impl Converter for BoundedImageConverter {
    fn convert(&self, request: &ConversionRequest) -> Result<Document, ConversionError> {
        convert(request, &self.limits)
    }
}

fn convert(request: &ConversionRequest, limits: &ImageLimits) -> Result<Document, ConversionError> {
    limits.validate()?;
    if request.limits.max_assets == 0 {
        return Err(limit_exceeded("assets", 1, 0));
    }
    let bytes = read_input(request)?;
    let parsed = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        parse_png(&bytes, limits)?
    } else if bytes.starts_with(&[0xff, 0xd8]) {
        parse_jpeg(&bytes, limits)?
    } else {
        return Err(ConversionError::UnsupportedFormat {
            format: "local image (PNG/JPEG only in v1)".into(),
        });
    };
    validate_dimensions(parsed.width, parsed.height, limits)?;
    let extension = request
        .source
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if !parsed.extensions.contains(&extension.as_str()) {
        return Err(ConversionError::CorruptInput {
            message: format!(
                "image signature indicates {}, but extension is {extension:?}",
                parsed.format
            ),
        });
    }
    let mut properties = parsed.metadata;
    properties.insert("width".into(), parsed.width.to_string());
    properties.insert("height".into(), parsed.height.to_string());
    properties.insert("ocr_policy".into(), "deferred_no_pixel_decode".into());
    let id = AssetId::new("asset-001")?;
    let title = request
        .source
        .file_name()
        .and_then(|value| value.to_str())
        .map(ToOwned::to_owned);
    let warnings = if parsed.semantic_text {
        Vec::new()
    } else {
        vec![ConversionWarning {
            code: WarningCode::OcrDeferred,
            message: "The image has no semantic text metadata; OCR was not run in local v1".into(),
            page: None,
        }]
    };
    Ok(Document {
        metadata: DocumentMetadata {
            title: title.clone(),
            source_format: Some(parsed.format.into()),
            properties,
            ..DocumentMetadata::default()
        },
        blocks: vec![Block::Image {
            asset_id: id.clone(),
            alt: title.unwrap_or_default(),
        }],
        assets: vec![Asset {
            id,
            file_name: format!("image-001.{}", parsed.canonical_extension),
            media_type: parsed.media_type.into(),
            data: bytes,
        }],
        warnings,
    })
}

fn validate_dimensions(
    width: u32,
    height: u32,
    limits: &ImageLimits,
) -> Result<(), ConversionError> {
    let width = u64::from(width);
    let height = u64::from(height);
    if width > limits.max_width {
        return Err(limit_exceeded("image_width", width, limits.max_width));
    }
    if height > limits.max_height {
        return Err(limit_exceeded("image_height", height, limits.max_height));
    }
    let pixels = width.saturating_mul(height);
    if pixels > limits.max_pixels {
        return Err(limit_exceeded("image_pixels", pixels, limits.max_pixels));
    }
    Ok(())
}

struct ParsedImage {
    format: &'static str,
    extensions: &'static [&'static str],
    canonical_extension: &'static str,
    media_type: &'static str,
    width: u32,
    height: u32,
    metadata: BTreeMap<String, String>,
    semantic_text: bool,
}

fn parse_png(bytes: &[u8], limits: &ImageLimits) -> Result<ParsedImage, ConversionError> {
    let mut cursor = 8usize;
    let mut chunks = 0u64;
    let mut metadata_bytes = 0u64;
    let mut width = None;
    let mut height = None;
    let mut idat = false;
    let mut idat_ended = false;
    let mut idat_data = Vec::new();
    let mut layout = None;
    let mut palette = false;
    let mut ended = false;
    let mut metadata = BTreeMap::new();
    let mut semantic_text = false;
    while cursor < bytes.len() {
        chunks = checked_counter(chunks, limits.max_chunks_or_segments, "image_chunks")?;
        let length = usize::try_from(read_be_u32(bytes, cursor)?)
            .map_err(|_| corrupt_error("PNG chunk length does not fit this platform"))?;
        let kind_start = checked_add(cursor, 4, "PNG chunk kind")?;
        let data_start = checked_add(kind_start, 4, "PNG chunk data")?;
        let data_end = checked_add(data_start, length, "PNG chunk data")?;
        let crc_end = checked_add(data_end, 4, "PNG chunk CRC")?;
        let kind = slice(bytes, kind_start, data_start, "PNG chunk kind")?;
        let data = slice(bytes, data_start, data_end, "PNG chunk data")?;
        let expected = read_be_u32(bytes, data_end)?;
        let mut hasher = Hasher::new();
        hasher.update(kind);
        hasher.update(data);
        if hasher.finalize() != expected {
            return Err(corrupt_error(format!(
                "PNG chunk {:?} has an invalid CRC",
                String::from_utf8_lossy(kind)
            )));
        }
        if idat && kind != b"IDAT" && kind != b"IEND" {
            idat_ended = true;
        }
        match kind {
            b"IHDR" => {
                if width.is_some() || chunks != 1 || data.len() != 13 {
                    return Err(corrupt_error("PNG must have one 13-byte first IHDR chunk"));
                }
                width = Some(u32::from_be_bytes(
                    data[0..4].try_into().expect("four bytes"),
                ));
                height = Some(u32::from_be_bytes(
                    data[4..8].try_into().expect("four bytes"),
                ));
                if width == Some(0) || height == Some(0) {
                    return Err(corrupt_error("PNG dimensions must be nonzero"));
                }
                validate_dimensions(
                    width.expect("set above"),
                    height.expect("set above"),
                    limits,
                )?;
                let bit_depth = data[8];
                let color_type = data[9];
                let channels = match (color_type, bit_depth) {
                    (0, 1 | 2 | 4 | 8 | 16) => 1u8,
                    (2, 8 | 16) => 3u8,
                    (3, 1 | 2 | 4 | 8) => 1u8,
                    (4, 8 | 16) => 2u8,
                    (6, 8 | 16) => 4u8,
                    _ => return Err(corrupt_error("unsupported PNG color type/bit depth")),
                };
                if data[10] != 0 || data[11] != 0 || data[12] != 0 {
                    return Err(corrupt_error(
                        "unsupported PNG compression, filter, or interlace method",
                    ));
                }
                layout = Some((bit_depth, channels, color_type));
            }
            b"PLTE" => {
                if idat || data.is_empty() || data.len() > 768 || data.len() % 3 != 0 {
                    return Err(corrupt_error("invalid PNG palette"));
                }
                palette = true;
            }
            b"IDAT" => {
                if width.is_none() || idat_ended {
                    return Err(corrupt_error(
                        "PNG IDAT chunks must be contiguous after IHDR",
                    ));
                }
                idat = true;
                idat_data.extend_from_slice(data);
            }
            b"tEXt" => {
                add_metadata_bytes(&mut metadata_bytes, data.len(), limits)?;
                let (key, value) = split_nul(data, "PNG tEXt")?;
                let key = latin1(key);
                let value = latin1(value);
                validate_png_keyword(&key)?;
                if semantic_png_keyword(&key) && !value.trim().is_empty() {
                    semantic_text = true;
                }
                metadata.insert(format!("png.{key}"), value);
            }
            b"zTXt" => {
                let (key, rest) = split_nul(data, "PNG zTXt")?;
                if rest.first() != Some(&0) {
                    return Err(corrupt_error("unsupported PNG zTXt compression method"));
                }
                let mut decoded = Vec::new();
                ZlibDecoder::new(&rest[1..])
                    .take(limits.max_metadata_bytes.saturating_add(1))
                    .read_to_end(&mut decoded)
                    .map_err(|error| corrupt_error(format!("invalid PNG zTXt stream: {error}")))?;
                add_metadata_bytes(&mut metadata_bytes, decoded.len(), limits)?;
                let key = latin1(key);
                validate_png_keyword(&key)?;
                let value = latin1(&decoded);
                if semantic_png_keyword(&key) && !value.trim().is_empty() {
                    semantic_text = true;
                }
                metadata.insert(format!("png.{key}"), value);
            }
            b"iTXt" => {
                add_metadata_bytes(&mut metadata_bytes, data.len(), limits)?;
                let keyword_end = data
                    .iter()
                    .position(|byte| *byte == 0)
                    .ok_or_else(|| corrupt_error("PNG iTXt is missing its keyword separator"))?;
                let control = data
                    .get(keyword_end + 1..keyword_end + 3)
                    .ok_or_else(|| corrupt_error("PNG iTXt is missing compression fields"))?;
                if control != [0, 0] {
                    return Err(corrupt_error(
                        "compressed or malformed PNG iTXt is unsupported",
                    ));
                }
                let rest = &data[keyword_end + 3..];
                let language_end = rest
                    .iter()
                    .position(|byte| *byte == 0)
                    .ok_or_else(|| corrupt_error("PNG iTXt is missing its language separator"))?;
                let translated = &rest[language_end + 1..];
                let translated_end =
                    translated
                        .iter()
                        .position(|byte| *byte == 0)
                        .ok_or_else(|| {
                            corrupt_error("PNG iTXt is missing its translated-keyword separator")
                        })?;
                let text = &translated[translated_end + 1..];
                let key = std::str::from_utf8(&data[..keyword_end]).map_err(|error| {
                    corrupt_error(format!("PNG iTXt keyword is not UTF-8: {error}"))
                })?;
                validate_png_keyword(key)?;
                let value = std::str::from_utf8(text).map_err(|error| {
                    corrupt_error(format!("PNG iTXt text is not UTF-8: {error}"))
                })?;
                if semantic_png_keyword(key) && !value.trim().is_empty() {
                    semantic_text = true;
                }
                metadata.insert(format!("png.{key}"), value.into());
            }
            b"IEND" => {
                if !data.is_empty() || crc_end != bytes.len() {
                    return Err(corrupt_error("PNG IEND is malformed or not final"));
                }
                ended = true;
            }
            _ => {
                if kind.first().is_some_and(u8::is_ascii_uppercase) && !matches!(kind, b"PLTE") {
                    return Err(corrupt_error(format!(
                        "unsupported critical PNG chunk {:?}",
                        String::from_utf8_lossy(kind)
                    )));
                }
            }
        }
        cursor = crc_end;
        if ended {
            break;
        }
    }
    if !ended || !idat {
        return Err(corrupt_error("PNG is missing IDAT or IEND"));
    }
    let (bit_depth, channels, color_type) =
        layout.ok_or_else(|| corrupt_error("PNG is missing IHDR layout"))?;
    if color_type == 3 && !palette {
        return Err(corrupt_error("indexed PNG is missing PLTE"));
    }
    let width_value = width.expect("validated IHDR width");
    let height_value = height.expect("validated IHDR height");
    let row_bits = u64::from(width_value)
        .checked_mul(u64::from(channels))
        .and_then(|value| value.checked_mul(u64::from(bit_depth)))
        .ok_or_else(|| corrupt_error("PNG row size overflow"))?;
    let row_bytes = row_bits
        .checked_add(7)
        .map(|value| value / 8)
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| corrupt_error("PNG row size overflow"))?;
    let expected = row_bytes
        .checked_mul(u64::from(height_value))
        .ok_or_else(|| corrupt_error("PNG image data size overflow"))?;
    let mut decoder = ZlibDecoder::new(idat_data.as_slice());
    let mut decoded = 0u64;
    let mut buffer = [0u8; 8192];
    loop {
        let read = decoder
            .read(&mut buffer)
            .map_err(|error| corrupt_error(format!("invalid PNG IDAT stream: {error}")))?;
        if read == 0 {
            break;
        }
        decoded = decoded
            .checked_add(u64::try_from(read).unwrap_or(u64::MAX))
            .ok_or_else(|| corrupt_error("PNG image data size overflow"))?;
        if decoded > expected {
            return Err(corrupt_error(
                "PNG IDAT expands beyond its declared dimensions",
            ));
        }
    }
    if decoded != expected
        || decoder.total_in() != u64::try_from(idat_data.len()).unwrap_or(u64::MAX)
    {
        return Err(corrupt_error(
            "PNG IDAT size or compressed-stream boundary is invalid",
        ));
    }
    Ok(ParsedImage {
        format: "png",
        extensions: &["png"],
        canonical_extension: "png",
        media_type: "image/png",
        width: width_value,
        height: height_value,
        metadata,
        semantic_text,
    })
}

fn parse_jpeg(bytes: &[u8], limits: &ImageLimits) -> Result<ParsedImage, ConversionError> {
    #[derive(Clone)]
    struct Frame {
        progressive: bool,
        components: Vec<u8>,
    }

    if !bytes.starts_with(&[0xff, 0xd8]) {
        return Err(corrupt_error("JPEG is missing SOI"));
    }
    let mut cursor = 2usize;
    let mut segments = 0u64;
    let mut metadata_bytes = 0u64;
    let mut dimensions = None;
    let mut frame: Option<Frame> = None;
    let mut metadata = BTreeMap::new();
    let mut comments = 0u64;
    let mut ended = false;
    let mut scans = 0u64;
    let mut sequential_components = std::collections::HashSet::new();
    let mut progressive_coefficients = std::collections::HashMap::new();
    while cursor < bytes.len() {
        if bytes.get(cursor) != Some(&0xff) {
            return Err(corrupt_error("JPEG marker is missing its 0xFF prefix"));
        }
        while bytes.get(cursor) == Some(&0xff) {
            cursor += 1;
        }
        let marker = *bytes
            .get(cursor)
            .ok_or_else(|| corrupt_error("truncated JPEG marker"))?;
        cursor += 1;
        if marker == 0xd9 {
            ended = cursor == bytes.len() && scans > 0;
            break;
        }
        if marker == 0xda {
            let frame = frame
                .as_ref()
                .ok_or_else(|| corrupt_error("JPEG scan precedes its frame"))?;
            scans = checked_counter(scans, limits.max_chunks_or_segments, "image_segments")?;
            let length = usize::from(read_be_u16(bytes, cursor)?);
            let data_start = checked_add(cursor, 2, "JPEG scan header")?;
            let data_end = checked_add(cursor, length, "JPEG scan header")?;
            let data = slice(bytes, data_start, data_end, "JPEG scan header")?;
            let component_count = usize::from(
                *data
                    .first()
                    .ok_or_else(|| corrupt_error("truncated JPEG scan header"))?,
            );
            if component_count == 0
                || component_count > frame.components.len()
                || data.len() != 4 + component_count * 2
            {
                return Err(corrupt_error("JPEG scan header length/components disagree"));
            }
            let mut selectors = std::collections::HashSet::new();
            for component in 0..component_count {
                let selector = data[1 + component * 2];
                let tables = data[2 + component * 2];
                if !frame.components.contains(&selector)
                    || !selectors.insert(selector)
                    || tables >> 4 > 3
                    || tables & 0x0f > 3
                {
                    return Err(corrupt_error("invalid JPEG scan component/table selector"));
                }
            }
            let parameters = &data[data.len() - 3..];
            let (spectral_start, spectral_end, approximation) =
                (parameters[0], parameters[1], parameters[2]);
            let high = approximation >> 4;
            let low = approximation & 0x0f;
            if frame.progressive {
                if spectral_start > spectral_end
                    || spectral_end > 63
                    || (spectral_start == 0 && spectral_end != 0)
                    || (spectral_start > 0 && component_count != 1)
                    || high > 13
                    || low > 13
                    || (high != 0 && high != low + 1)
                {
                    return Err(corrupt_error("invalid progressive JPEG scan parameters"));
                }
                for selector in &selectors {
                    for coefficient in spectral_start..=spectral_end {
                        let key = (*selector, coefficient);
                        if high == 0 {
                            if progressive_coefficients.insert(key, low).is_some() {
                                return Err(corrupt_error(
                                    "duplicate initial progressive JPEG coefficient scan",
                                ));
                            }
                        } else if progressive_coefficients.get(&key) != Some(&high) {
                            return Err(corrupt_error(
                                "progressive JPEG refinement has no matching prior scan",
                            ));
                        } else {
                            progressive_coefficients.insert(key, low);
                        }
                    }
                }
            } else if spectral_start != 0 || spectral_end != 63 || high != 0 || low != 0 {
                return Err(corrupt_error("invalid sequential JPEG scan parameters"));
            } else if selectors
                .iter()
                .any(|selector| !sequential_components.insert(*selector))
            {
                return Err(corrupt_error(
                    "sequential JPEG component appears in multiple scans",
                ));
            }
            cursor = data_end;
            let mut entropy_bytes = 0u64;
            loop {
                let byte = *bytes
                    .get(cursor)
                    .ok_or_else(|| corrupt_error("truncated JPEG entropy data"))?;
                if byte != 0xff {
                    entropy_bytes = entropy_bytes.saturating_add(1);
                    cursor += 1;
                    continue;
                }
                let marker_start = cursor;
                while bytes.get(cursor) == Some(&0xff) {
                    cursor += 1;
                }
                let next = *bytes
                    .get(cursor)
                    .ok_or_else(|| corrupt_error("truncated JPEG entropy marker"))?;
                if next == 0x00 {
                    if cursor - marker_start != 1 {
                        return Err(corrupt_error(
                            "JPEG entropy byte stuffing must be exactly 0xFF00",
                        ));
                    }
                    entropy_bytes = entropy_bytes.saturating_add(1);
                    cursor += 1;
                } else if (0xd0..=0xd7).contains(&next) {
                    cursor += 1;
                } else {
                    cursor = marker_start;
                    break;
                }
            }
            if entropy_bytes == 0 {
                return Err(corrupt_error("JPEG scan has no entropy-coded data"));
            }
            continue;
        }
        if marker == 0x01 {
            continue;
        }
        if marker == 0x00 || matches!(marker, 0xd0..=0xd8) {
            return Err(corrupt_error("invalid standalone JPEG marker position"));
        }
        segments = checked_counter(segments, limits.max_chunks_or_segments, "image_segments")?;
        let length = usize::from(read_be_u16(bytes, cursor)?);
        if length < 2 {
            return Err(corrupt_error("invalid JPEG segment length"));
        }
        let data_start = checked_add(cursor, 2, "JPEG segment")?;
        let data_end = checked_add(cursor, length, "JPEG segment")?;
        let data = slice(bytes, data_start, data_end, "JPEG segment")?;
        if matches!(marker, 0xc0 | 0xc2) {
            if frame.is_some() {
                return Err(corrupt_error(
                    "JPEG contains multiple supported frame headers",
                ));
            }
            let component_count = usize::from(
                *data
                    .get(5)
                    .ok_or_else(|| corrupt_error("truncated JPEG frame header"))?,
            );
            if data.first() != Some(&8)
                || !(1..=4).contains(&component_count)
                || data.len() != 6 + component_count * 3
            {
                return Err(corrupt_error("JPEG frame length/components disagree"));
            }
            let height = u32::from(u16::from_be_bytes([data[1], data[2]]));
            let width = u32::from(u16::from_be_bytes([data[3], data[4]]));
            let mut components = Vec::with_capacity(component_count);
            for component in 0..component_count {
                let offset = 6 + component * 3;
                let id = data[offset];
                let sampling = data[offset + 1];
                if components.contains(&id)
                    || sampling >> 4 == 0
                    || sampling >> 4 > 4
                    || sampling & 0x0f == 0
                    || sampling & 0x0f > 4
                    || data[offset + 2] > 3
                {
                    return Err(corrupt_error("invalid JPEG frame component descriptor"));
                }
                components.push(id);
            }
            dimensions = Some((width, height));
            frame = Some(Frame {
                progressive: marker == 0xc2,
                components,
            });
        } else if matches!(marker, 0xc1 | 0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            return Err(ConversionError::UnsupportedFormat {
                format: format!("JPEG coding marker 0x{marker:02x}"),
            });
        } else if marker == 0xfe {
            add_metadata_bytes(&mut metadata_bytes, data.len(), limits)?;
            comments = comments.checked_add(1).ok_or_else(|| {
                limit_exceeded("image_metadata_bytes", u64::MAX, limits.max_metadata_bytes)
            })?;
            let value = String::from_utf8_lossy(data).into_owned();
            metadata.insert(
                if comments == 1 {
                    "jpeg.comment".into()
                } else {
                    format!("jpeg.comment.{comments}")
                },
                value,
            );
        } else if marker == 0xe1 && data.starts_with(b"Exif\0\0") {
            add_metadata_bytes(&mut metadata_bytes, data.len(), limits)?;
            parse_exif(&data[6..], &mut metadata)?;
        }
        cursor = data_end;
    }
    if !ended || scans == 0 {
        return Err(corrupt_error("JPEG is truncated or has trailing bytes"));
    }
    let frame = frame.expect("a scan cannot exist without a frame");
    if frame.progressive {
        if frame
            .components
            .iter()
            .any(|component| !progressive_coefficients.contains_key(&(*component, 0)))
        {
            return Err(corrupt_error(
                "progressive JPEG is missing an initial DC scan",
            ));
        }
    } else if sequential_components.len() != frame.components.len() {
        return Err(corrupt_error(
            "sequential JPEG does not scan every frame component",
        ));
    }
    let (width, height) =
        dimensions.ok_or_else(|| corrupt_error("JPEG contains no supported frame dimensions"))?;
    if width == 0 || height == 0 {
        return Err(corrupt_error("JPEG dimensions must be nonzero"));
    }
    let semantic_text = metadata.iter().any(|(key, value)| {
        (key.starts_with("jpeg.comment") || key == "exif.description") && !value.trim().is_empty()
    });
    Ok(ParsedImage {
        format: "jpeg",
        extensions: &["jpg", "jpeg"],
        canonical_extension: "jpg",
        media_type: "image/jpeg",
        width,
        height,
        metadata,
        semantic_text,
    })
}

fn parse_exif(
    bytes: &[u8],
    metadata: &mut BTreeMap<String, String>,
) -> Result<(), ConversionError> {
    if bytes.len() < 8 {
        return Err(corrupt_error("truncated EXIF TIFF header"));
    }
    let little = match &bytes[..2] {
        b"II" => true,
        b"MM" => false,
        _ => return Err(corrupt_error("invalid EXIF byte order")),
    };
    if tiff_u16(bytes, 2, little)? != 42 {
        return Err(corrupt_error("invalid EXIF TIFF marker"));
    }
    let offset = usize::try_from(tiff_u32(bytes, 4, little)?)
        .map_err(|_| corrupt_error("EXIF IFD offset does not fit this platform"))?;
    let count = usize::from(tiff_u16(bytes, offset, little)?);
    for index in 0..count {
        let entry = checked_add(
            offset + 2,
            index
                .checked_mul(12)
                .ok_or_else(|| corrupt_error("EXIF entry offset overflow"))?,
            "EXIF entry",
        )?;
        let tag = tiff_u16(bytes, entry, little)?;
        let kind = tiff_u16(bytes, entry + 2, little)?;
        let count = usize::try_from(tiff_u32(bytes, entry + 4, little)?)
            .map_err(|_| corrupt_error("EXIF value length does not fit this platform"))?;
        if kind != 2 || count == 0 {
            continue;
        }
        let start = if count <= 4 {
            entry + 8
        } else {
            usize::try_from(tiff_u32(bytes, entry + 8, little)?)
                .map_err(|_| corrupt_error("EXIF value offset does not fit this platform"))?
        };
        let value = slice(
            bytes,
            start,
            checked_add(start, count, "EXIF value")?,
            "EXIF value",
        )?;
        let value = std::str::from_utf8(value.strip_suffix(&[0]).unwrap_or(value))
            .map_err(|error| corrupt_error(format!("EXIF text is not UTF-8: {error}")))?;
        let key = match tag {
            0x010e => "exif.description",
            0x010f => "exif.make",
            0x0110 => "exif.model",
            0x0132 => "exif.datetime",
            _ => continue,
        };
        metadata.insert(key.into(), value.into());
    }
    Ok(())
}

fn checked_counter(current: u64, maximum: u64, name: &'static str) -> Result<u64, ConversionError> {
    let actual = current.saturating_add(1);
    if actual > maximum {
        return Err(limit_exceeded(name, actual, maximum));
    }
    Ok(actual)
}

fn add_metadata_bytes(
    current: &mut u64,
    bytes: usize,
    limits: &ImageLimits,
) -> Result<(), ConversionError> {
    let actual = current
        .checked_add(u64::try_from(bytes).unwrap_or(u64::MAX))
        .unwrap_or(u64::MAX);
    if actual > limits.max_metadata_bytes {
        return Err(limit_exceeded(
            "image_metadata_bytes",
            actual,
            limits.max_metadata_bytes,
        ));
    }
    *current = actual;
    Ok(())
}

fn split_nul<'a>(bytes: &'a [u8], context: &str) -> Result<(&'a [u8], &'a [u8]), ConversionError> {
    let index = bytes
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| corrupt_error(format!("{context} is missing a separator")))?;
    Ok((&bytes[..index], &bytes[index + 1..]))
}

fn validate_png_keyword(keyword: &str) -> Result<(), ConversionError> {
    if keyword.is_empty() || keyword.len() > 79 || keyword.trim() != keyword {
        return Err(corrupt_error("invalid PNG text keyword"));
    }
    Ok(())
}

fn semantic_png_keyword(keyword: &str) -> bool {
    matches!(
        keyword.to_ascii_lowercase().as_str(),
        "title" | "description" | "comment" | "caption" | "subject" | "author"
    )
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| char::from(*byte)).collect()
}

fn read_be_u16(bytes: &[u8], offset: usize) -> Result<u16, ConversionError> {
    let value = slice(bytes, offset, checked_add(offset, 2, "u16")?, "u16")?;
    Ok(u16::from_be_bytes([value[0], value[1]]))
}

fn read_be_u32(bytes: &[u8], offset: usize) -> Result<u32, ConversionError> {
    let value = slice(bytes, offset, checked_add(offset, 4, "u32")?, "u32")?;
    Ok(u32::from_be_bytes([value[0], value[1], value[2], value[3]]))
}

fn tiff_u16(bytes: &[u8], offset: usize, little: bool) -> Result<u16, ConversionError> {
    let value = slice(
        bytes,
        offset,
        checked_add(offset, 2, "EXIF u16")?,
        "EXIF u16",
    )?;
    Ok(if little {
        u16::from_le_bytes([value[0], value[1]])
    } else {
        u16::from_be_bytes([value[0], value[1]])
    })
}

fn tiff_u32(bytes: &[u8], offset: usize, little: bool) -> Result<u32, ConversionError> {
    let value = slice(
        bytes,
        offset,
        checked_add(offset, 4, "EXIF u32")?,
        "EXIF u32",
    )?;
    Ok(if little {
        u32::from_le_bytes(value.try_into().expect("four bytes"))
    } else {
        u32::from_be_bytes(value.try_into().expect("four bytes"))
    })
}

fn checked_add(left: usize, right: usize, context: &str) -> Result<usize, ConversionError> {
    left.checked_add(right)
        .ok_or_else(|| corrupt_error(format!("integer overflow while reading {context}")))
}

fn slice<'a>(
    bytes: &'a [u8],
    start: usize,
    end: usize,
    context: &str,
) -> Result<&'a [u8], ConversionError> {
    bytes
        .get(start..end)
        .ok_or_else(|| corrupt_error(format!("truncated {context}")))
}

fn corrupt_error(message: impl Into<String>) -> ConversionError {
    ConversionError::CorruptInput {
        message: message.into(),
    }
}
