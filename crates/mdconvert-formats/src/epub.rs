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

const CONTAINER_NS: &str = "urn:oasis:names:tc:opendocument:xmlns:container";
const OPF_NS: &str = "http://www.idpf.org/2007/opf";
const DC_NS: &str = "http://purl.org/dc/elements/1.1/";
const XHTML_NS: &str = "http://www.w3.org/1999/xhtml";

#[derive(Debug, Default, Clone, Copy)]
pub struct EpubConverter;

#[derive(Clone)]
struct ManifestItem {
    path: String,
    media_type: String,
    properties: HashSet<String>,
}

struct EpubAssets {
    assets: Vec<Asset>,
    refs: HashMap<String, AssetId>,
    by_part: HashMap<String, String>,
    total_bytes: u64,
}

impl EpubAssets {
    fn new() -> Self {
        Self {
            assets: Vec::new(),
            refs: HashMap::new(),
            by_part: HashMap::new(),
            total_bytes: 0,
        }
    }

    fn reference(
        &mut self,
        archive: &Archive,
        part: &str,
        media_type: &str,
        request: &ConversionRequest,
    ) -> Result<String, ConversionError> {
        if let Some(source) = self.by_part.get(part) {
            return Ok(source.clone());
        }
        let actual = u64::try_from(self.assets.len())
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        if actual > u64::from(request.limits.max_assets) {
            return Err(ConversionError::LimitExceeded {
                limit: "assets",
                actual,
                maximum: u64::from(request.limits.max_assets),
            });
        }
        let entry = archive.entry(part)?;
        let bytes = u64::try_from(entry.data.len()).unwrap_or(u64::MAX);
        let total = self.total_bytes.saturating_add(bytes);
        if total > request.limits.max_input_bytes {
            return Err(ConversionError::LimitExceeded {
                limit: "asset_bytes",
                actual: total,
                maximum: request.limits.max_input_bytes,
            });
        }
        let extension = crate::image::validate_embedded_image(&entry.data, media_type)?;
        let id = AssetId::new(format!("epub-image-{actual:03}"))?;
        let source = format!("mdconvert-asset:epub-{actual:03}");
        self.refs.insert(source.clone(), id.clone());
        self.by_part.insert(part.to_owned(), source.clone());
        self.assets.push(Asset {
            id,
            file_name: format!("image-{actual:03}.{extension}"),
            media_type: media_type.to_owned(),
            data: entry.data.clone(),
        });
        self.total_bytes = total;
        Ok(source)
    }
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
        if !container.roots[0].is(CONTAINER_NS, "container") {
            return Err(corrupt_error(
                "EPUB container root has the wrong expanded name",
            ));
        }
        let rootfile = container.roots[0]
            .descendants_ns(CONTAINER_NS, "rootfile")
            .find(|node| {
                node.attr_ns(None, "media-type")
                    .is_none_or(|value| value == "application/oebps-package+xml")
            })
            .ok_or_else(|| corrupt_error("EPUB container has no package rootfile"))?;
        let opf_path = required_attr(rootfile, "full-path", "EPUB rootfile")?;
        let opf_path = package_root_path(opf_path)?;
        let opf = parse_xml_bytes(&archive.entry(&opf_path)?.data, &opf_path)?;
        let package = &opf.roots[0];
        if !package.is(OPF_NS, "package") {
            return Err(corrupt_error("EPUB package root must be package"));
        }
        let metadata_node = package.child_ns(OPF_NS, "metadata");
        let title = metadata_node
            .and_then(|node| node.descendants_ns(DC_NS, "title").next())
            .map(XmlNode::text)
            .filter(|value| !value.trim().is_empty());
        let author = metadata_node
            .and_then(|node| node.descendants_ns(DC_NS, "creator").next())
            .map(XmlNode::text)
            .filter(|value| !value.trim().is_empty());
        let manifest_node = package
            .child_ns(OPF_NS, "manifest")
            .ok_or_else(|| corrupt_error("EPUB package has no manifest"))?;
        let mut manifest = HashMap::new();
        let mut manifest_by_path = HashMap::new();
        let mut nav = None;
        for item in manifest_node
            .children()
            .filter(|node| node.is(OPF_NS, "item"))
        {
            let id = required_attr(item, "id", "EPUB manifest item")?.to_owned();
            if manifest.contains_key(&id) {
                return Err(corrupt_error(format!("duplicate EPUB manifest ID {id:?}")));
            }
            let href = required_attr(item, "href", "EPUB manifest item")?;
            let path = resolve_package_path(&opf_path, href)?;
            archive.entry(&path)?;
            let manifest_item = ManifestItem {
                path: path.clone(),
                media_type: required_attr(item, "media-type", "EPUB manifest item")?.to_owned(),
                properties: item
                    .attr_ns(None, "properties")
                    .unwrap_or("")
                    .split_whitespace()
                    .map(ToOwned::to_owned)
                    .collect(),
            };
            if manifest_by_path
                .insert(path.clone(), manifest_item.clone())
                .is_some()
            {
                return Err(corrupt_error(format!(
                    "duplicate EPUB manifest path {path:?}"
                )));
            }
            if manifest_item.properties.contains("nav")
                && nav.replace(manifest_item.clone()).is_some()
            {
                return Err(corrupt_error("EPUB manifest contains multiple nav items"));
            }
            manifest.insert(id, manifest_item);
        }
        let spine_node = package
            .child_ns(OPF_NS, "spine")
            .ok_or_else(|| corrupt_error("EPUB package has no spine"))?;
        let mut spine = Vec::new();
        for itemref in spine_node
            .children()
            .filter(|node| node.is(OPF_NS, "itemref"))
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
        let mut assets = EpubAssets::new();
        let mut warnings = Vec::new();
        if let Some(nav) = nav.as_ref() {
            blocks.push(Block::Heading {
                level: 2,
                content: vec![Inline::Text("Navigation".into())],
            });
            let mut converted =
                convert_xhtml(&archive, nav, &manifest_by_path, request, &mut assets)?;
            blocks.append(&mut converted.blocks);
            warnings.append(&mut converted.warnings);
        }
        for item in &spine {
            let mut converted =
                convert_xhtml(&archive, item, &manifest_by_path, request, &mut assets)?;
            blocks.append(&mut converted.blocks);
            warnings.append(&mut converted.warnings);
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
            assets: assets.assets,
            warnings,
        })
    }
}

fn convert_xhtml(
    archive: &Archive,
    item: &ManifestItem,
    manifest: &HashMap<String, ManifestItem>,
    request: &ConversionRequest,
    assets: &mut EpubAssets,
) -> Result<Document, ConversionError> {
    let entry = archive.entry(&item.path)?;
    let parsed = parse_xml_bytes(&entry.data, &item.path)?;
    if !parsed.roots[0].is(XHTML_NS, "html") || !only_xhtml_elements(&parsed.roots[0]) {
        return Err(corrupt_error(format!(
            "EPUB XHTML {:?} contains a foreign expanded element name",
            item.path
        )));
    }
    let sanitized = sanitize_xhtml(&entry.data, &item.path, archive, manifest, request, assets)?;
    let mut embedded_request = request.clone();
    embedded_request.source_url = Some(
        Url::parse(&format!("epub://local/{}", percent_path(&item.path))).map_err(|error| {
            ConversionError::ConversionFailed {
                message: format!("could not build local EPUB base URL: {error}"),
            }
        })?,
    );
    let converted =
        HtmlConverter.convert_bytes_with_asset_refs(&sanitized, &embedded_request, &assets.refs)?;
    if !converted.assets.is_empty() {
        return Err(corrupt_error(
            "EPUB XHTML created an unauthenticated HTML-owned asset",
        ));
    }
    Ok(converted)
}

fn only_xhtml_elements(node: &XmlNode) -> bool {
    node.namespace_uri() == Some(XHTML_NS) && node.children().all(only_xhtml_elements)
}

fn sanitize_xhtml(
    bytes: &[u8],
    part: &str,
    archive: &Archive,
    manifest: &HashMap<String, ManifestItem>,
    request: &ConversionRequest,
    assets: &mut EpubAssets,
) -> Result<Vec<u8>, ConversionError> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().enable_all_checks(true);
    let mut writer = Writer::new(Vec::new());
    loop {
        let event = reader
            .read_event()
            .map_err(|error| corrupt_error(format!("invalid EPUB XHTML {part:?}: {error}")))?;
        match event {
            Event::Start(start) => {
                let rebuilt =
                    sanitize_start(&reader, &start, part, archive, manifest, request, assets)?;
                writer
                    .write_event(Event::Start(rebuilt))
                    .map_err(io_error)?;
            }
            Event::Empty(start) => {
                let rebuilt =
                    sanitize_start(&reader, &start, part, archive, manifest, request, assets)?;
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
    manifest: &HashMap<String, ManifestItem>,
    request: &ConversionRequest,
    assets: &mut EpubAssets,
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
                return Err(corrupt_error(format!(
                    "EPUB image source {value:?} is not a package-local reference"
                )));
            }
            let raw_path = value.split('#').next().unwrap_or("");
            if raw_path.trim().is_empty() {
                return Err(corrupt_error("EPUB image source has no package part"));
            }
            let path = resolve_package_path(part, raw_path)?;
            let item = manifest.get(&path).ok_or_else(|| {
                corrupt_error(format!(
                    "EPUB image {path:?} is not authenticated by the manifest"
                ))
            })?;
            assets.reference(archive, &path, &item.media_type, request)?
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

fn package_root_path(path: &str) -> Result<String, ConversionError> {
    let normalized = path.replace('\\', "/");
    if normalized.contains('\0')
        || normalized.starts_with('/')
        || normalized.starts_with("//")
        || normalized.contains(':')
        || (normalized.as_bytes().get(1) == Some(&b':')
            && normalized.as_bytes()[0].is_ascii_alphabetic())
    {
        return Err(corrupt_error(format!("unsafe EPUB rootfile path {path:?}")));
    }
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
