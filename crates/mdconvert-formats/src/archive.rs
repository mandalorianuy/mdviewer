use std::{collections::HashSet, io::Read, path::Path};

use crc32fast::Hasher;
use flate2::read::DeflateDecoder;
use mdconvert_core::{
    Asset, AssetId, Block, ConversionError, ConversionRequest, ConversionWarning, Converter,
    Document, DocumentMetadata, Inline, ListItem, WarningCode,
};
use mdconvert_html::HtmlConverter;

use crate::{
    StructuredLimits,
    csv::convert_csv_bytes,
    json::convert_json_bytes,
    limit_exceeded, read_input,
    xml::{XmlNode, convert_xml_bytes, parse_xml},
};

const LOCAL_HEADER: u32 = 0x0403_4b50;
const CENTRAL_HEADER: u32 = 0x0201_4b50;
const EOCD: u32 = 0x0605_4b50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveLimits {
    pub max_entries: u64,
    pub max_entry_compressed_bytes: u64,
    pub max_entry_uncompressed_bytes: u64,
    pub max_total_uncompressed_bytes: u64,
    pub max_expansion_ratio: u64,
}

impl Default for ArchiveLimits {
    fn default() -> Self {
        Self {
            max_entries: 10_000,
            max_entry_compressed_bytes: 128 * 1024 * 1024,
            max_entry_uncompressed_bytes: 256 * 1024 * 1024,
            max_total_uncompressed_bytes: 500 * 1024 * 1024,
            max_expansion_ratio: 200,
        }
    }
}

impl ArchiveLimits {
    pub(crate) fn validate(&self) -> Result<(), ConversionError> {
        for (name, value) in [
            ("max_entries", self.max_entries),
            (
                "max_entry_compressed_bytes",
                self.max_entry_compressed_bytes,
            ),
            (
                "max_entry_uncompressed_bytes",
                self.max_entry_uncompressed_bytes,
            ),
            (
                "max_total_uncompressed_bytes",
                self.max_total_uncompressed_bytes,
            ),
            ("max_expansion_ratio", self.max_expansion_ratio),
        ] {
            if value == 0 {
                return Err(ConversionError::ConversionFailed {
                    message: format!("archive limit {name} must be greater than zero"),
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ZipConverter;

impl ZipConverter {
    pub fn with_limits(limits: ArchiveLimits) -> BoundedZipConverter {
        BoundedZipConverter { limits }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BoundedZipConverter {
    limits: ArchiveLimits,
}

impl Converter for ZipConverter {
    fn convert(&self, request: &ConversionRequest) -> Result<Document, ConversionError> {
        convert_zip(request, &ArchiveLimits::default())
    }
}

impl Converter for BoundedZipConverter {
    fn convert(&self, request: &ConversionRequest) -> Result<Document, ConversionError> {
        convert_zip(request, &self.limits)
    }
}

fn convert_zip(
    request: &ConversionRequest,
    limits: &ArchiveLimits,
) -> Result<Document, ConversionError> {
    let archive = Archive::open(request, limits)?;
    if let Some(entry) = archive.entries.iter().find(|entry| is_archive(&entry.data)) {
        return Err(ConversionError::UnsupportedFormat {
            format: format!("nested archive entry {}", entry.name),
        });
    }
    let mut entries: Vec<_> = archive.entries.iter().collect();
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    let convertible = entries
        .iter()
        .filter_map(|entry| inner_format(&entry.name).map(|format| (*entry, format)))
        .collect::<Vec<_>>();
    if let Some((selected, format)) = convertible.first().copied() {
        let mut inner_request = request.clone();
        inner_request.source = Path::new(&selected.name).to_path_buf();
        inner_request.source_url = url::Url::parse(&format!("zip://local/{}", selected.name)).ok();
        inner_request.limits.max_input_bytes = inner_request
            .limits
            .max_input_bytes
            .min(u64::try_from(selected.data.len()).unwrap_or(u64::MAX));
        let mut document = match format {
            "csv" => {
                convert_csv_bytes(&inner_request, &selected.data, &StructuredLimits::default())
            }
            "json" => {
                convert_json_bytes(&inner_request, &selected.data, &StructuredLimits::default())
            }
            "xml" => {
                convert_xml_bytes(&inner_request, &selected.data, &StructuredLimits::default())
            }
            "html" => HtmlConverter.convert_bytes(&selected.data, &inner_request),
            _ => unreachable!("inner_format returned an unsupported format"),
        }?;
        document.metadata.source_format = Some(format!("zip ({format})"));
        document
            .metadata
            .properties
            .insert("selected_entry".into(), selected.name.clone());
        document
            .metadata
            .properties
            .insert("entry_count".into(), archive.entries.len().to_string());
        document.metadata.properties.insert(
            "convertible_entry_count".into(),
            convertible.len().to_string(),
        );
        if convertible.len() > 1 {
            let skipped = convertible
                .iter()
                .skip(1)
                .map(|(entry, _)| entry.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            document.warnings.push(ConversionWarning {
                code: WarningCode::AdditionalArchiveEntriesSkipped,
                message: format!(
                    "converted {}; additional supported archive entries skipped: {skipped}",
                    selected.name
                ),
                page: None,
            });
        }
        return Ok(document);
    }
    let items = entries
        .into_iter()
        .map(|entry| ListItem {
            blocks: vec![Block::Paragraph {
                content: vec![Inline::Text(entry.name.clone())],
            }],
        })
        .collect();
    Ok(Document {
        metadata: DocumentMetadata {
            source_format: Some("zip".into()),
            properties: [("entry_count".into(), archive.entries.len().to_string())]
                .into_iter()
                .collect(),
            ..DocumentMetadata::default()
        },
        blocks: vec![Block::List {
            ordered: false,
            start: None,
            items,
        }],
        assets: Vec::new(),
        warnings: Vec::new(),
    })
}

fn inner_format(name: &str) -> Option<&'static str> {
    match Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())?
        .to_ascii_lowercase()
        .as_str()
    {
        "csv" => Some("csv"),
        "json" => Some("json"),
        "xml" => Some("xml"),
        "html" | "htm" => Some("html"),
        _ => None,
    }
}

fn is_archive(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06")
}

#[derive(Debug)]
pub(crate) struct Archive {
    pub(crate) entries: Vec<ArchiveEntry>,
    pub(crate) first_entry_name: String,
}

#[derive(Debug)]
pub(crate) struct ArchiveEntry {
    pub(crate) name: String,
    pub(crate) data: Vec<u8>,
    pub(crate) compression: u16,
}

impl Archive {
    pub(crate) fn open(
        request: &ConversionRequest,
        limits: &ArchiveLimits,
    ) -> Result<Self, ConversionError> {
        limits.validate()?;
        let bytes = read_input(request)?;
        Self::parse(&bytes, limits)
    }

    pub(crate) fn entry(&self, name: &str) -> Result<&ArchiveEntry, ConversionError> {
        self.entries
            .iter()
            .find(|entry| entry.name == name)
            .ok_or_else(|| ConversionError::CorruptInput {
                message: format!("archive entry {name:?} is missing"),
            })
    }

    pub(crate) fn optional(&self, name: &str) -> Option<&ArchiveEntry> {
        self.entries.iter().find(|entry| entry.name == name)
    }

    fn parse(bytes: &[u8], limits: &ArchiveLimits) -> Result<Self, ConversionError> {
        let eocd_offset = find_eocd(bytes)?;
        let disk = read_u16(bytes, eocd_offset + 4)?;
        let central_disk = read_u16(bytes, eocd_offset + 6)?;
        let entries_disk = read_u16(bytes, eocd_offset + 8)?;
        let entries = read_u16(bytes, eocd_offset + 10)?;
        if disk != 0 || central_disk != 0 || entries_disk != entries {
            return corrupt("multi-disk ZIP archives are unsupported");
        }
        if entries == u16::MAX {
            return corrupt("ZIP64 archives are unsupported");
        }
        let entry_count = u64::from(entries);
        if entry_count > limits.max_entries {
            return Err(limit_exceeded(
                "archive_entries",
                entry_count,
                limits.max_entries,
            ));
        }
        let central_size = usize::try_from(read_u32(bytes, eocd_offset + 12)?)
            .map_err(|_| corrupt_error("central directory size does not fit this platform"))?;
        let central_offset = usize::try_from(read_u32(bytes, eocd_offset + 16)?)
            .map_err(|_| corrupt_error("central directory offset does not fit this platform"))?;
        let central_end = checked_add(central_offset, central_size, "central directory")?;
        if central_end != eocd_offset || central_end > bytes.len() {
            return corrupt("corrupt or overlapping ZIP central directory");
        }

        let mut cursor = central_offset;
        let mut names = HashSet::new();
        let mut descriptors = Vec::with_capacity(usize::from(entries));
        let mut total = 0u64;
        for _ in 0..entries {
            if read_u32(bytes, cursor)? != CENTRAL_HEADER {
                return corrupt("invalid ZIP central directory signature");
            }
            let made_by = read_u16(bytes, cursor + 4)?;
            let flags = read_u16(bytes, cursor + 8)?;
            let compression = read_u16(bytes, cursor + 10)?;
            validate_flags(flags)?;
            if !matches!(compression, 0 | 8) {
                return corrupt(format!("unsupported ZIP compression method {compression}"));
            }
            let crc = read_u32(bytes, cursor + 16)?;
            let compressed = u64::from(read_u32(bytes, cursor + 20)?);
            let uncompressed = u64::from(read_u32(bytes, cursor + 24)?);
            if compressed == u64::from(u32::MAX) || uncompressed == u64::from(u32::MAX) {
                return corrupt("ZIP64 entry sizes are unsupported");
            }
            check_entry_limits(compressed, uncompressed, limits)?;
            total = total.checked_add(uncompressed).ok_or_else(|| {
                limit_exceeded(
                    "archive_total_uncompressed_bytes",
                    u64::MAX,
                    limits.max_total_uncompressed_bytes,
                )
            })?;
            if total > limits.max_total_uncompressed_bytes {
                return Err(limit_exceeded(
                    "archive_total_uncompressed_bytes",
                    total,
                    limits.max_total_uncompressed_bytes,
                ));
            }
            let name_len = usize::from(read_u16(bytes, cursor + 28)?);
            let extra_len = usize::from(read_u16(bytes, cursor + 30)?);
            let comment_len = usize::from(read_u16(bytes, cursor + 32)?);
            let external = read_u32(bytes, cursor + 38)?;
            let local_offset = usize::try_from(read_u32(bytes, cursor + 42)?)
                .map_err(|_| corrupt_error("local header offset does not fit this platform"))?;
            let name_start = checked_add(cursor, 46, "central entry")?;
            let name_end = checked_add(name_start, name_len, "central entry name")?;
            let extra_end = checked_add(name_end, extra_len, "central entry extra")?;
            let next = checked_add(extra_end, comment_len, "central entry comment")?;
            let raw_name = slice(bytes, name_start, name_end, "central entry name")?;
            validate_extra(slice(bytes, name_end, extra_end, "central entry extra")?)?;
            let name = decode_name(raw_name, flags)?;
            let normalized = normalize_name(&name)?;
            if !names.insert(normalized.clone()) {
                return corrupt(format!("duplicate normalized archive entry {normalized:?}"));
            }
            let is_dir = normalized.ends_with('/');
            validate_entry_kind(made_by, external, is_dir)?;
            descriptors.push(Descriptor {
                name: normalized,
                raw_name: raw_name.to_vec(),
                flags,
                compression,
                crc,
                compressed,
                uncompressed,
                local_offset,
                is_dir,
            });
            cursor = next;
        }
        if cursor != central_end {
            return corrupt("central directory entry count or size is inconsistent");
        }

        let first_entry_name = descriptors
            .first()
            .map(|descriptor| descriptor.name.clone())
            .ok_or_else(|| corrupt_error("ZIP archive contains no entries"))?;
        let mut output = Vec::with_capacity(descriptors.len());
        for descriptor in descriptors {
            if descriptor.is_dir {
                continue;
            }
            output.push(extract(bytes, &descriptor, limits)?);
        }
        Ok(Self {
            entries: output,
            first_entry_name,
        })
    }
}

pub(crate) fn parse_xml_bytes(
    bytes: &[u8],
    label: &str,
) -> Result<crate::xml::ParsedXml, ConversionError> {
    let input = std::str::from_utf8(bytes).map_err(|error| ConversionError::CorruptInput {
        message: format!("{label} is not valid UTF-8 XML: {error}"),
    })?;
    parse_xml(input, &StructuredLimits::default()).map_err(|error| match error {
        ConversionError::CorruptInput { message } => ConversionError::CorruptInput {
            message: format!("invalid {label}: {message}"),
        },
        other => other,
    })
}

#[derive(Debug, Clone)]
pub(crate) struct Relationship {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) target: String,
    pub(crate) external: bool,
}

pub(crate) fn relationships(
    archive: &Archive,
    path: &str,
) -> Result<Vec<Relationship>, ConversionError> {
    let Some(entry) = archive.optional(path) else {
        return Ok(Vec::new());
    };
    let parsed = parse_xml_bytes(&entry.data, path)?;
    let root = parsed
        .roots
        .first()
        .ok_or_else(|| corrupt_error("empty relationships XML"))?;
    let mut output = Vec::new();
    let mut ids = HashSet::new();
    for node in root.descendants("Relationship") {
        let id = required_attr(node, "Id", path)?;
        if !ids.insert(id.to_owned()) {
            return corrupt(format!("duplicate relationship ID {id:?} in {path}"));
        }
        output.push(Relationship {
            id: id.to_owned(),
            kind: required_attr(node, "Type", path)?.to_owned(),
            target: required_attr(node, "Target", path)?.to_owned(),
            external: node
                .attr("TargetMode")
                .is_some_and(|value| value.eq_ignore_ascii_case("external")),
        });
    }
    Ok(output)
}

pub(crate) fn required_attr<'a>(
    node: &'a XmlNode,
    name: &str,
    label: &str,
) -> Result<&'a str, ConversionError> {
    node.attr(name)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| corrupt_error(format!("{label} element is missing attribute {name}")))
}

pub(crate) fn resolve_package_path(
    base_part: &str,
    target: &str,
) -> Result<String, ConversionError> {
    if target.contains('\0') || target.starts_with('/') || target.starts_with("//") {
        return corrupt(format!("unsafe package relationship target {target:?}"));
    }
    let target = target.replace('\\', "/");
    if target.contains(':') {
        return corrupt(format!(
            "package relationship target has a scheme {target:?}"
        ));
    }
    let mut parts: Vec<&str> = base_part
        .rsplit_once('/')
        .map_or(Vec::new(), |(directory, _)| directory.split('/').collect());
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return corrupt(format!("escaping package relationship target {target:?}"));
                }
            }
            value => parts.push(value),
        }
    }
    if parts.is_empty() {
        return corrupt("package relationship target resolves to an empty path");
    }
    Ok(parts.join("/"))
}

pub(crate) struct AssetSink {
    assets: Vec<Asset>,
    by_part: std::collections::HashMap<String, AssetId>,
}

impl AssetSink {
    pub(crate) fn new() -> Self {
        Self {
            assets: Vec::new(),
            by_part: std::collections::HashMap::new(),
        }
    }

    pub(crate) fn add(
        &mut self,
        archive: &Archive,
        part: &str,
        request: &ConversionRequest,
    ) -> Result<AssetId, ConversionError> {
        if let Some(id) = self.by_part.get(part) {
            return Ok(id.clone());
        }
        let actual = u64::try_from(self.assets.len())
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        if actual > u64::from(request.limits.max_assets) {
            return Err(limit_exceeded(
                "assets",
                actual,
                u64::from(request.limits.max_assets),
            ));
        }
        let entry = archive.entry(part)?;
        let extension = safe_extension(part)
            .ok_or_else(|| corrupt_error(format!("unsupported embedded image type in {part:?}")))?;
        let id = AssetId::new(format!("asset-{actual:03}"))?;
        self.assets.push(Asset {
            id: id.clone(),
            file_name: format!("image-{actual:03}-{}", file_leaf(part)),
            media_type: media_type(extension).into(),
            data: entry.data.clone(),
        });
        self.by_part.insert(part.to_owned(), id.clone());
        Ok(id)
    }

    pub(crate) fn into_assets(self) -> Vec<Asset> {
        self.assets
    }
}

fn file_leaf(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or("image.bin")
}

fn safe_extension(path: &str) -> Option<&str> {
    match path.rsplit_once('.')?.1.to_ascii_lowercase().as_str() {
        "png" => Some("png"),
        "jpg" | "jpeg" => Some("jpg"),
        "gif" => Some("gif"),
        "webp" => Some("webp"),
        "bmp" => Some("bmp"),
        "svg" => Some("svg"),
        _ => None,
    }
}

fn media_type(extension: &str) -> &'static str {
    match extension {
        "png" => "image/png",
        "jpg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

struct Descriptor {
    name: String,
    raw_name: Vec<u8>,
    flags: u16,
    compression: u16,
    crc: u32,
    compressed: u64,
    uncompressed: u64,
    local_offset: usize,
    is_dir: bool,
}

fn find_eocd(bytes: &[u8]) -> Result<usize, ConversionError> {
    if bytes.len() < 22 {
        return corrupt("truncated ZIP end record");
    }
    let start = bytes.len().saturating_sub(22 + usize::from(u16::MAX));
    for offset in (start..=bytes.len() - 22).rev() {
        if bytes.get(offset..offset + 4) == Some(EOCD.to_le_bytes().as_slice()) {
            let comment = usize::from(read_u16(bytes, offset + 20)?);
            if checked_add(offset, 22 + comment, "ZIP comment")? == bytes.len() {
                return Ok(offset);
            }
        }
    }
    corrupt("ZIP end record is missing or corrupt")
}

fn validate_flags(flags: u16) -> Result<(), ConversionError> {
    if flags & 1 != 0 || flags & (1 << 6) != 0 {
        return Err(ConversionError::EncryptedInput);
    }
    let supported = (1 << 3) | (1 << 11);
    if flags & !supported != 0 {
        return corrupt(format!(
            "unsupported ZIP general-purpose flags 0x{flags:04x}"
        ));
    }
    Ok(())
}

fn validate_extra(mut extra: &[u8]) -> Result<(), ConversionError> {
    while !extra.is_empty() {
        if extra.len() < 4 {
            return corrupt("truncated ZIP extra field");
        }
        let id = u16::from_le_bytes([extra[0], extra[1]]);
        let size = usize::from(u16::from_le_bytes([extra[2], extra[3]]));
        if extra.len() < 4 + size {
            return corrupt("truncated ZIP extra field payload");
        }
        if matches!(id, 0x0001 | 0x9901) {
            return corrupt("ZIP64 and AES extra fields are unsupported");
        }
        extra = &extra[4 + size..];
    }
    Ok(())
}

fn decode_name(bytes: &[u8], flags: u16) -> Result<String, ConversionError> {
    if flags & (1 << 11) == 0 && !bytes.is_ascii() {
        return corrupt("non-ASCII ZIP names require the UTF-8 flag");
    }
    std::str::from_utf8(bytes)
        .map(ToOwned::to_owned)
        .map_err(|error| corrupt_error(format!("ZIP entry name is not UTF-8: {error}")))
}

fn normalize_name(name: &str) -> Result<String, ConversionError> {
    if name.contains('\0') {
        return corrupt("archive entry name contains NUL");
    }
    let name = name.replace('\\', "/");
    if name.starts_with('/') || name.starts_with("//") {
        return corrupt(format!("absolute or UNC archive path {name:?}"));
    }
    if name.as_bytes().get(1).is_some_and(|byte| *byte == b':')
        && name.as_bytes()[0].is_ascii_alphabetic()
    {
        return corrupt(format!("drive-qualified archive path {name:?}"));
    }
    let directory = name.ends_with('/');
    let mut parts = Vec::new();
    for part in name.split('/') {
        match part {
            "" if directory && parts.is_empty() => {
                return corrupt("archive entry name is empty");
            }
            "" => continue,
            "." => {}
            ".." => return corrupt(format!("traversing archive path {name:?}")),
            value => parts.push(value),
        }
    }
    if parts.is_empty() {
        return corrupt("archive entry name is empty");
    }
    let mut normalized = parts.join("/");
    if directory {
        normalized.push('/');
    }
    Ok(normalized)
}

fn validate_entry_kind(made_by: u16, external: u32, is_dir: bool) -> Result<(), ConversionError> {
    let creator = made_by >> 8;
    if creator == 3 {
        let mode = external >> 16;
        let kind = mode & 0o170000;
        if kind != 0 && kind != 0o100000 && kind != 0o040000 {
            return corrupt("symlink or special ZIP entry is unsupported");
        }
        if (kind == 0o040000) != is_dir && kind != 0 {
            return corrupt("ZIP directory metadata disagrees with its name");
        }
    }
    if external & 0x10 != 0 && !is_dir {
        return corrupt("ZIP directory attribute disagrees with its name");
    }
    Ok(())
}

fn check_entry_limits(
    compressed: u64,
    uncompressed: u64,
    limits: &ArchiveLimits,
) -> Result<(), ConversionError> {
    if compressed > limits.max_entry_compressed_bytes {
        return Err(limit_exceeded(
            "archive_entry_compressed_bytes",
            compressed,
            limits.max_entry_compressed_bytes,
        ));
    }
    if uncompressed > limits.max_entry_uncompressed_bytes {
        return Err(limit_exceeded(
            "archive_entry_uncompressed_bytes",
            uncompressed,
            limits.max_entry_uncompressed_bytes,
        ));
    }
    let ratio_limit = compressed.saturating_mul(limits.max_expansion_ratio);
    if uncompressed > ratio_limit && uncompressed > 1024 {
        return Err(limit_exceeded(
            "archive_expansion_ratio",
            uncompressed,
            ratio_limit,
        ));
    }
    Ok(())
}

fn extract(
    bytes: &[u8],
    descriptor: &Descriptor,
    limits: &ArchiveLimits,
) -> Result<ArchiveEntry, ConversionError> {
    let offset = descriptor.local_offset;
    if read_u32(bytes, offset)? != LOCAL_HEADER {
        return corrupt(format!("invalid local header for {}", descriptor.name));
    }
    let flags = read_u16(bytes, offset + 6)?;
    let compression = read_u16(bytes, offset + 8)?;
    if flags != descriptor.flags || compression != descriptor.compression {
        return corrupt(format!(
            "central and local headers disagree for {}",
            descriptor.name
        ));
    }
    if flags & (1 << 3) == 0 {
        let local_crc = read_u32(bytes, offset + 14)?;
        let local_compressed = u64::from(read_u32(bytes, offset + 18)?);
        let local_uncompressed = u64::from(read_u32(bytes, offset + 22)?);
        if local_crc != descriptor.crc
            || local_compressed != descriptor.compressed
            || local_uncompressed != descriptor.uncompressed
        {
            return corrupt(format!(
                "central and local sizes or CRC disagree for {}",
                descriptor.name
            ));
        }
    }
    let name_len = usize::from(read_u16(bytes, offset + 26)?);
    let extra_len = usize::from(read_u16(bytes, offset + 28)?);
    let name_start = checked_add(offset, 30, "local header")?;
    let name_end = checked_add(name_start, name_len, "local name")?;
    let extra_end = checked_add(name_end, extra_len, "local extra")?;
    if slice(bytes, name_start, name_end, "local name")? != descriptor.raw_name {
        return corrupt(format!(
            "central and local names disagree for {}",
            descriptor.name
        ));
    }
    validate_extra(slice(bytes, name_end, extra_end, "local extra")?)?;
    let compressed_len = usize::try_from(descriptor.compressed)
        .map_err(|_| corrupt_error("compressed entry size does not fit this platform"))?;
    let data_end = checked_add(extra_end, compressed_len, "entry data")?;
    let compressed = slice(bytes, extra_end, data_end, "entry data")?;
    let maximum = descriptor
        .uncompressed
        .min(limits.max_entry_uncompressed_bytes);
    let mut data = Vec::with_capacity(usize::try_from(maximum.min(64 * 1024)).unwrap_or(0));
    match descriptor.compression {
        0 => data.extend_from_slice(compressed),
        8 => {
            let mut decoder = DeflateDecoder::new(compressed);
            {
                decoder
                    .by_ref()
                    .take(maximum.saturating_add(1))
                    .read_to_end(&mut data)
                    .map_err(|error| corrupt_error(format!("invalid deflate stream: {error}")))?;
            }
            if decoder.total_in() != descriptor.compressed {
                return corrupt(format!(
                    "deflate stream for {} has trailing or unread compressed bytes",
                    descriptor.name
                ));
            }
        }
        _ => unreachable!("validated compression method"),
    }
    let actual = u64::try_from(data.len()).unwrap_or(u64::MAX);
    if actual > maximum {
        return Err(limit_exceeded(
            "archive_entry_uncompressed_bytes",
            actual,
            maximum,
        ));
    }
    if actual != descriptor.uncompressed {
        return corrupt(format!(
            "uncompressed size mismatch for {}: expected {}, received {actual}",
            descriptor.name, descriptor.uncompressed
        ));
    }
    let mut hasher = Hasher::new();
    hasher.update(&data);
    if hasher.finalize() != descriptor.crc {
        return corrupt(format!("CRC mismatch for {}", descriptor.name));
    }
    Ok(ArchiveEntry {
        name: descriptor.name.clone(),
        data,
        compression: descriptor.compression,
    })
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, ConversionError> {
    let value = slice(bytes, offset, checked_add(offset, 2, "u16")?, "u16")?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, ConversionError> {
    let value = slice(bytes, offset, checked_add(offset, 4, "u32")?, "u32")?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
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
        .ok_or_else(|| corrupt_error(format!("truncated ZIP {context}")))
}

fn corrupt<T>(message: impl Into<String>) -> Result<T, ConversionError> {
    Err(corrupt_error(message))
}

fn corrupt_error(message: impl Into<String>) -> ConversionError {
    ConversionError::CorruptInput {
        message: message.into(),
    }
}
