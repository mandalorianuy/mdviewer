use std::collections::{HashMap, HashSet};

use mdconvert_core::{
    Asset, AssetId, Block, ConversionError, ConversionRequest, Converter, Document,
    DocumentMetadata, Inline,
};
use mdconvert_html::HtmlConverter;
use quick_xml::{
    Reader, Writer, XmlVersion,
    events::{BytesStart, Event},
};
use url::Url;

use crate::{
    archive::{Archive, ArchiveLimits, parse_xml_bytes, required_attr, resolve_package_path},
    xml::XmlNode,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct EpubConverter;

#[derive(Clone)]
struct ManifestItem {
    path: String,
    media_type: String,
    properties: HashSet<String>,
}

impl Converter for EpubConverter {
    fn convert(&self, request: &ConversionRequest) -> Result<Document, ConversionError> {
        let archive = Archive::open(request, &ArchiveLimits::default())?;
        let mimetype = archive.entry("mimetype")?;
        if archive.first_entry_name != "mimetype"
            || mimetype.compression != 0
            || mimetype.data != b"application/epub+zip"
        {
            return Err(corrupt_error(
                "EPUB mimetype must be the first stored entry and exactly application/epub+zip",
            ));
        }
        let container = parse_xml_bytes(
            &archive.entry("META-INF/container.xml")?.data,
            "META-INF/container.xml",
        )?;
        let rootfile = container.roots[0]
            .descendants("rootfile")
            .find(|node| {
                node.attr("media-type")
                    .is_none_or(|value| value == "application/oebps-package+xml")
            })
            .ok_or_else(|| corrupt_error("EPUB container has no package rootfile"))?;
        let opf_path = required_attr(rootfile, "full-path", "EPUB rootfile")?;
        let opf_path = package_root_path(opf_path)?;
        let opf = parse_xml_bytes(&archive.entry(&opf_path)?.data, &opf_path)?;
        let package = &opf.roots[0];
        if package.local_name() != "package" {
            return Err(corrupt_error("EPUB package root must be package"));
        }
        let metadata_node = package.child("metadata");
        let title = metadata_node
            .and_then(|node| node.descendants("title").next())
            .map(XmlNode::text)
            .filter(|value| !value.trim().is_empty());
        let author = metadata_node
            .and_then(|node| node.descendants("creator").next())
            .map(XmlNode::text)
            .filter(|value| !value.trim().is_empty());
        let manifest_node = package
            .child("manifest")
            .ok_or_else(|| corrupt_error("EPUB package has no manifest"))?;
        let mut manifest = HashMap::new();
        for item in manifest_node
            .children()
            .filter(|node| node.local_name() == "item")
        {
            let id = required_attr(item, "id", "EPUB manifest item")?.to_owned();
            if manifest.contains_key(&id) {
                return Err(corrupt_error(format!("duplicate EPUB manifest ID {id:?}")));
            }
            let href = required_attr(item, "href", "EPUB manifest item")?;
            let path = resolve_package_path(&opf_path, href)?;
            archive.entry(&path)?;
            manifest.insert(
                id.clone(),
                ManifestItem {
                    path,
                    media_type: required_attr(item, "media-type", "EPUB manifest item")?.to_owned(),
                    properties: item
                        .attr("properties")
                        .unwrap_or("")
                        .split_whitespace()
                        .map(ToOwned::to_owned)
                        .collect(),
                },
            );
        }
        let spine_node = package
            .child("spine")
            .ok_or_else(|| corrupt_error("EPUB package has no spine"))?;
        let mut spine = Vec::new();
        for itemref in spine_node
            .children()
            .filter(|node| node.local_name() == "itemref")
        {
            let id = required_attr(itemref, "idref", "EPUB spine item")?;
            let item = manifest.get(id).ok_or_else(|| {
                corrupt_error(format!("EPUB spine references missing manifest ID {id:?}"))
            })?;
            if item.media_type != "application/xhtml+xml" {
                return Err(corrupt_error(format!(
                    "EPUB spine item {id:?} is not XHTML"
                )));
            }
            spine.push(item.clone());
        }
        if spine.is_empty() {
            return Err(corrupt_error("EPUB spine is empty"));
        }
        let count = u64::try_from(spine.len()).unwrap_or(u64::MAX);
        if count > u64::from(request.limits.max_pages) {
            return Err(ConversionError::LimitExceeded {
                limit: "pages",
                actual: count,
                maximum: u64::from(request.limits.max_pages),
            });
        }

        let mut blocks = Vec::new();
        let mut assets = Vec::new();
        let mut warnings = Vec::new();
        if let Some(nav) = manifest
            .values()
            .find(|item| item.properties.contains("nav"))
        {
            blocks.push(Block::Heading {
                level: 2,
                content: vec![Inline::Text("Navigation".into())],
            });
            let mut converted = convert_xhtml(&archive, nav, request)?;
            merge_document(
                &mut blocks,
                &mut assets,
                &mut warnings,
                &mut converted,
                request,
            )?;
        }
        for item in &spine {
            let mut converted = convert_xhtml(&archive, item, request)?;
            merge_document(
                &mut blocks,
                &mut assets,
                &mut warnings,
                &mut converted,
                request,
            )?;
        }
        Ok(Document {
            metadata: DocumentMetadata {
                title,
                author,
                source_format: Some("epub".into()),
                page_count: Some(u32::try_from(spine.len()).unwrap_or(u32::MAX)),
                properties: [
                    ("spine_order".into(), "package".into()),
                    ("external_resolution".into(), "disabled".into()),
                ]
                .into_iter()
                .collect(),
                ..DocumentMetadata::default()
            },
            blocks,
            assets,
            warnings,
        })
    }
}

fn convert_xhtml(
    archive: &Archive,
    item: &ManifestItem,
    request: &ConversionRequest,
) -> Result<Document, ConversionError> {
    let entry = archive.entry(&item.path)?;
    parse_xml_bytes(&entry.data, &item.path)?;
    let sanitized = sanitize_xhtml(&entry.data, &item.path, archive)?;
    let mut embedded_request = request.clone();
    embedded_request.source_url = Some(
        Url::parse(&format!("epub://local/{}", percent_path(&item.path))).map_err(|error| {
            ConversionError::ConversionFailed {
                message: format!("could not build local EPUB base URL: {error}"),
            }
        })?,
    );
    HtmlConverter.convert_bytes(&sanitized, &embedded_request)
}

fn sanitize_xhtml(bytes: &[u8], part: &str, archive: &Archive) -> Result<Vec<u8>, ConversionError> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().enable_all_checks(true);
    let mut writer = Writer::new(Vec::new());
    loop {
        let event = reader
            .read_event()
            .map_err(|error| corrupt_error(format!("invalid EPUB XHTML {part:?}: {error}")))?;
        match event {
            Event::Start(start) => {
                let rebuilt = sanitize_start(&reader, &start, part, archive)?;
                writer
                    .write_event(Event::Start(rebuilt))
                    .map_err(io_error)?;
            }
            Event::Empty(start) => {
                let rebuilt = sanitize_start(&reader, &start, part, archive)?;
                writer
                    .write_event(Event::Empty(rebuilt))
                    .map_err(io_error)?;
            }
            Event::DocType(_) => return Err(corrupt_error("EPUB XHTML DTDs are unsupported")),
            Event::Eof => break,
            other => writer.write_event(other).map_err(io_error)?,
        }
    }
    Ok(writer.into_inner())
}

fn sanitize_start(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    part: &str,
    archive: &Archive,
) -> Result<BytesStart<'static>, ConversionError> {
    let name = std::str::from_utf8(start.name().as_ref())
        .map_err(|error| corrupt_error(format!("EPUB XHTML element name is not UTF-8: {error}")))?
        .to_owned();
    let local = name
        .rsplit_once(':')
        .map_or(name.as_str(), |(_, local)| local)
        .to_owned();
    let mut rebuilt = BytesStart::new(name);
    for attribute in start.attributes() {
        let attribute = attribute
            .map_err(|error| corrupt_error(format!("invalid EPUB XHTML attribute: {error}")))?;
        let key = std::str::from_utf8(attribute.key.as_ref())
            .map_err(|error| {
                corrupt_error(format!("EPUB XHTML attribute name is not UTF-8: {error}"))
            })?
            .to_owned();
        let value = attribute
            .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
            .map_err(|error| corrupt_error(format!("invalid EPUB XHTML attribute value: {error}")))?
            .into_owned();
        let key_local = key
            .rsplit_once(':')
            .map_or(key.as_str(), |(_, local)| local);
        if key_local.starts_with("on") || (local == "base" && key_local == "href") {
            continue;
        }
        let value = if local == "a" && key_local == "href" {
            if external_reference(&value) {
                continue;
            }
            value
        } else if local == "img" && key_local == "src" {
            if external_reference(&value) {
                value
            } else {
                let path = resolve_package_path(part, value.split('#').next().unwrap_or(""))?;
                let image = archive.entry(&path)?;
                data_url(&path, &image.data)?
            }
        } else {
            value
        };
        rebuilt.push_attribute((key.as_str(), value.as_str()));
    }
    Ok(rebuilt.into_owned())
}

fn external_reference(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with("//")
        || trimmed.split_once(':').is_some_and(|(scheme, _)| {
            scheme.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
            })
        })
}

fn data_url(path: &str, bytes: &[u8]) -> Result<String, ConversionError> {
    let media = match path
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        _ => {
            return Err(corrupt_error(format!(
                "unsupported EPUB image type {path:?}"
            )));
        }
    };
    let mut value = format!("data:{media},");
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut value, "%{byte:02X}").expect("writing to a string cannot fail");
    }
    Ok(value)
}

fn merge_document(
    blocks: &mut Vec<Block>,
    assets: &mut Vec<Asset>,
    warnings: &mut Vec<mdconvert_core::ConversionWarning>,
    document: &mut Document,
    request: &ConversionRequest,
) -> Result<(), ConversionError> {
    let mut remap = HashMap::new();
    for asset in document.assets.drain(..) {
        let replacement = if let Some(existing) = assets
            .iter()
            .find(|existing| existing.media_type == asset.media_type && existing.data == asset.data)
        {
            existing.id.clone()
        } else {
            let actual = assets
                .len()
                .checked_add(1)
                .ok_or(ConversionError::LimitExceeded {
                    limit: "assets",
                    actual: u64::MAX,
                    maximum: u64::MAX - 1,
                })?;
            if u64::try_from(actual).unwrap_or(u64::MAX) > u64::from(request.limits.max_assets) {
                return Err(ConversionError::LimitExceeded {
                    limit: "assets",
                    actual: u64::try_from(actual).unwrap_or(u64::MAX),
                    maximum: u64::from(request.limits.max_assets),
                });
            }
            let id = AssetId::new(format!("asset-{actual:03}"))?;
            assets.push(Asset {
                id: id.clone(),
                file_name: format!(
                    "image-{actual:03}.{}",
                    extension_for_media(&asset.media_type)
                ),
                media_type: asset.media_type,
                data: asset.data,
            });
            id
        };
        remap.insert(asset.id.as_str().to_owned(), replacement);
    }
    for block in &mut document.blocks {
        remap_block(block, &remap);
    }
    blocks.append(&mut document.blocks);
    warnings.append(&mut document.warnings);
    Ok(())
}

fn remap_block(block: &mut Block, remap: &HashMap<String, AssetId>) {
    match block {
        Block::Image { asset_id, .. } => {
            if let Some(replacement) = remap.get(asset_id.as_str()) {
                *asset_id = replacement.clone();
            }
        }
        Block::List { items, .. } => {
            for item in items {
                for block in &mut item.blocks {
                    remap_block(block, remap);
                }
            }
        }
        Block::Quote { blocks } => {
            for block in blocks {
                remap_block(block, remap);
            }
        }
        _ => {}
    }
}

fn extension_for_media(media: &str) -> &'static str {
    match media {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        _ => "bin",
    }
}

fn package_root_path(path: &str) -> Result<String, ConversionError> {
    if path.contains('\0') || path.starts_with('/') || path.starts_with("//") || path.contains(':')
    {
        return Err(corrupt_error(format!("unsafe EPUB rootfile path {path:?}")));
    }
    let normalized = path.replace('\\', "/");
    if normalized.split('/').any(|part| part == "..") {
        return Err(corrupt_error(format!(
            "traversing EPUB rootfile path {path:?}"
        )));
    }
    Ok(normalized
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>()
        .join("/"))
}

fn percent_path(path: &str) -> String {
    path.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'-' | b'_') {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

fn io_error(error: std::io::Error) -> ConversionError {
    ConversionError::ConversionFailed {
        message: format!("could not sanitize EPUB XHTML: {error}"),
    }
}

fn corrupt_error(message: impl Into<String>) -> ConversionError {
    ConversionError::CorruptInput {
        message: message.into(),
    }
}
