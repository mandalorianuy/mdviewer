use std::collections::{BTreeMap, HashMap, HashSet};

use mdconvert_core::{
    Alignment, Block, ConversionError, ConversionRequest, ConversionWarning, Converter, Document,
    DocumentMetadata, Inline, ListItem, WarningCode,
};

use crate::{
    archive::{
        Archive, ArchiveLimits, AssetSink, ContentTypes, Relationship, authenticate_ooxml,
        parse_xml_bytes, relationships, resolve_package_path,
    },
    xml::XmlNode,
};

const DOCX_MAIN_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
const IMAGE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const A_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const CP_NS: &str = "http://schemas.openxmlformats.org/package/2006/metadata/core-properties";
const DC_NS: &str = "http://purl.org/dc/elements/1.1/";

#[derive(Debug, Default, Clone, Copy)]
pub struct DocxConverter;

impl Converter for DocxConverter {
    fn convert(&self, request: &ConversionRequest) -> Result<Document, ConversionError> {
        let archive = Archive::open(request, &ArchiveLimits::default())?;
        let content_types =
            authenticate_ooxml(&archive, "word/document.xml", DOCX_MAIN_CONTENT_TYPE)?;
        let document = parse_xml_bytes(
            &archive.entry("word/document.xml")?.data,
            "word/document.xml",
        )?;
        let root = document
            .roots
            .first()
            .ok_or_else(|| corrupt_error("DOCX document XML is empty"))?;
        if !root.is(W_NS, "document") {
            return Err(corrupt_error(
                "DOCX document root has the wrong expanded name",
            ));
        }
        let body = root
            .child_ns(W_NS, "body")
            .ok_or_else(|| corrupt_error("DOCX has no document body"))?;
        let styles = parse_styles(&archive)?;
        let numbering = parse_numbering(&archive)?;
        let rels = relationships(&archive, "word/_rels/document.xml.rels")?
            .into_iter()
            .map(|relationship| (relationship.id.clone(), relationship))
            .collect::<HashMap<_, _>>();
        let mut warnings = Vec::new();
        let mut assets = AssetSink::new();
        let mut context = DocxContext {
            rels: &rels,
            archive: &archive,
            request,
            content_types: &content_types,
            assets: &mut assets,
            warnings: &mut warnings,
        };
        let mut blocks = Vec::new();
        let children: Vec<_> = body.children().collect();
        let mut index = 0usize;
        while index < children.len() {
            if children[index].is(W_NS, "p") {
                let paragraph =
                    convert_paragraph(children[index], &styles, &numbering, &mut context)?;
                if paragraph.list.is_some() {
                    let mut paragraphs = Vec::new();
                    while index < children.len() && children[index].is(W_NS, "p") {
                        let candidate =
                            convert_paragraph(children[index], &styles, &numbering, &mut context)?;
                        if candidate.list.is_none() {
                            break;
                        }
                        paragraphs.push(candidate);
                        index += 1;
                    }
                    let mut cursor = 0usize;
                    while cursor < paragraphs.len() {
                        let level = paragraphs[cursor].list.unwrap().level;
                        blocks.push(build_list(&paragraphs, &mut cursor, level));
                    }
                    continue;
                }
                blocks.extend(paragraph.into_blocks());
            } else if children[index].is(W_NS, "tbl") {
                blocks.extend(convert_table(children[index], &mut context)?);
            }
            index += 1;
        }
        let metadata = core_metadata(&archive)?;
        Ok(Document {
            metadata: DocumentMetadata {
                source_format: Some("docx".into()),
                ..metadata
            },
            blocks,
            assets: assets.into_assets(),
            warnings,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct ListKind {
    level: u32,
    ordered: bool,
    start: u64,
}

struct Paragraph {
    content: Vec<Inline>,
    heading: Option<u8>,
    list: Option<ListKind>,
    images: Vec<mdconvert_core::AssetId>,
}

struct DocxContext<'a> {
    rels: &'a HashMap<String, Relationship>,
    archive: &'a Archive,
    request: &'a ConversionRequest,
    content_types: &'a ContentTypes,
    assets: &'a mut AssetSink,
    warnings: &'a mut Vec<ConversionWarning>,
}

type Numbering = HashMap<(String, u32), (bool, u64)>;

impl Paragraph {
    fn into_blocks(self) -> Vec<Block> {
        let mut blocks = Vec::new();
        if !self.content.is_empty() {
            blocks.push(if let Some(level) = self.heading {
                Block::Heading {
                    level,
                    content: self.content,
                }
            } else {
                Block::Paragraph {
                    content: self.content,
                }
            });
        }
        blocks.extend(self.images.into_iter().map(|asset_id| Block::Image {
            asset_id,
            alt: String::new(),
        }));
        blocks
    }
}

fn convert_paragraph(
    node: &XmlNode,
    styles: &HashMap<String, u8>,
    numbering: &Numbering,
    context: &mut DocxContext<'_>,
) -> Result<Paragraph, ConversionError> {
    let properties = node.child_ns(W_NS, "pPr");
    let heading = properties
        .and_then(|value| value.child_ns(W_NS, "pStyle"))
        .and_then(|value| value.attr_ns(Some(W_NS), "val"))
        .and_then(|style| styles.get(style).copied())
        .or_else(|| {
            properties
                .and_then(|value| value.child_ns(W_NS, "outlineLvl"))
                .and_then(|value| value.attr_ns(Some(W_NS), "val"))
                .and_then(|value| value.parse::<u8>().ok())
                .and_then(|value| value.checked_add(1))
                .filter(|value| *value <= 6)
        });
    let list = properties
        .and_then(|value| value.child_ns(W_NS, "numPr"))
        .and_then(|numbering_properties| {
            let level = numbering_properties
                .child_ns(W_NS, "ilvl")?
                .attr_ns(Some(W_NS), "val")?
                .parse::<u32>()
                .ok()?;
            let id = numbering_properties
                .child_ns(W_NS, "numId")?
                .attr_ns(Some(W_NS), "val")?;
            numbering
                .get(&(id.to_owned(), level))
                .map(|(ordered, start)| ListKind {
                    level,
                    ordered: *ordered,
                    start: *start,
                })
        });
    let mut content = Vec::new();
    let mut images = Vec::new();
    for child in node.children().filter(|child| !child.is(W_NS, "pPr")) {
        append_inline_content(child, context, &mut content, &mut images)?;
    }
    Ok(Paragraph {
        content,
        heading,
        list,
        images,
    })
}

fn append_inline_content(
    node: &XmlNode,
    context: &mut DocxContext<'_>,
    output: &mut Vec<Inline>,
    images: &mut Vec<mdconvert_core::AssetId>,
) -> Result<(), ConversionError> {
    if node.is(W_NS, "r") {
        let mut run = Vec::new();
        for child in node.children().filter(|child| !child.is(W_NS, "rPr")) {
            if child.is(W_NS, "t") || child.is(W_NS, "delText") || child.is(W_NS, "instrText") {
                run.push(Inline::Text(child.text()));
            } else if child.is(W_NS, "tab") {
                run.push(Inline::Text("\t".into()));
            } else if child.is(W_NS, "br") || child.is(W_NS, "cr") {
                run.push(Inline::LineBreak);
            } else if child.is(W_NS, "drawing") || child.is(W_NS, "pict") {
                for blip in child.descendants_ns(A_NS, "blip") {
                    if let Some(id) = blip.attr_ns(Some(R_NS), "embed") {
                        images.push(add_related_image(id, context)?);
                    }
                }
            } else {
                append_inline_content(child, context, &mut run, images)?;
            }
        }
        let properties = node.child_ns(W_NS, "rPr");
        if properties.is_some_and(|value| value.child_ns(W_NS, "b").is_some()) {
            run = vec![Inline::Strong(run)];
        }
        if properties.is_some_and(|value| value.child_ns(W_NS, "i").is_some()) {
            run = vec![Inline::Emphasis(run)];
        }
        output.extend(run);
    } else if node.is(W_NS, "hyperlink") {
        let mut label = Vec::new();
        for child in node.children() {
            append_inline_content(child, context, &mut label, images)?;
        }
        let anchor = node
            .attr_ns(Some(W_NS), "anchor")
            .map(|value| format!("#{value}"));
        let relationship = node
            .attr_ns(Some(R_NS), "id")
            .and_then(|id| context.rels.get(id));
        let target = anchor.or_else(|| relationship.map(|value| value.target.clone()));
        if relationship.is_some_and(|value| value.external) {
            push_warning(
                context.warnings,
                WarningCode::ExternalLinkSkipped,
                "External DOCX hyperlink relationship was preserved as text".into(),
            );
            output.extend(label);
        } else if let Some(target) = target.as_ref().filter(|target| safe_link(target)) {
            output.push(Inline::Link {
                url: target.clone(),
                title: None,
                content: label,
            });
        } else {
            if target.is_some() {
                push_warning(
                    context.warnings,
                    WarningCode::InvalidLinkSkipped,
                    "Unsafe DOCX hyperlink was preserved as text".into(),
                );
            }
            output.extend(label);
        }
    } else {
        for child in node.children() {
            append_inline_content(child, context, output, images)?;
        }
    }
    Ok(())
}

fn push_warning(warnings: &mut Vec<ConversionWarning>, code: WarningCode, message: String) {
    if !warnings
        .iter()
        .any(|warning| warning.code == code && warning.message == message && warning.page.is_none())
    {
        warnings.push(ConversionWarning {
            code,
            message,
            page: None,
        });
    }
}

fn add_related_image(
    id: &str,
    context: &mut DocxContext<'_>,
) -> Result<mdconvert_core::AssetId, ConversionError> {
    let relationship = context.rels.get(id).ok_or_else(|| {
        corrupt_error(format!("DOCX image references missing relationship {id:?}"))
    })?;
    if relationship.external {
        context.warnings.push(ConversionWarning {
            code: WarningCode::ExternalAssetSkipped,
            message: format!("External DOCX image relationship {id:?} was skipped"),
            page: None,
        });
        return Err(corrupt_error(
            "external DOCX image cannot be represented as a local asset",
        ));
    }
    if relationship.kind != IMAGE_REL {
        return Err(corrupt_error(format!(
            "DOCX image relationship {id:?} has invalid type {:?}",
            relationship.kind
        )));
    }
    let path = resolve_package_path("word/document.xml", &relationship.target)?;
    context.assets.add(
        context.archive,
        &path,
        context.request,
        context.content_types,
    )
}

fn safe_link(target: &str) -> bool {
    target.starts_with('#')
        || (!target.contains(':')
            && !target.starts_with('/')
            && !target
                .replace('\\', "/")
                .split('/')
                .any(|part| part == ".."))
}

fn build_list(paragraphs: &[Paragraph], cursor: &mut usize, level: u32) -> Block {
    let kind = paragraphs[*cursor].list.expect("list paragraph");
    let mut items: Vec<ListItem> = Vec::new();
    while *cursor < paragraphs.len() {
        let current = paragraphs[*cursor].list.expect("list paragraph");
        if current.level < level || (current.level == level && current.ordered != kind.ordered) {
            break;
        }
        if current.level > level {
            if let Some(last) = items.last_mut() {
                last.blocks
                    .push(build_list(paragraphs, cursor, current.level));
                continue;
            }
            break;
        }
        let paragraph = &paragraphs[*cursor];
        let mut blocks = Vec::new();
        if !paragraph.content.is_empty() {
            blocks.push(Block::Paragraph {
                content: paragraph.content.clone(),
            });
        }
        blocks.extend(
            paragraph
                .images
                .iter()
                .cloned()
                .map(|asset_id| Block::Image {
                    asset_id,
                    alt: String::new(),
                }),
        );
        items.push(ListItem { blocks });
        *cursor += 1;
    }
    Block::List {
        ordered: kind.ordered,
        start: kind.ordered.then_some(kind.start),
        items,
    }
}

fn convert_table(
    table: &XmlNode,
    context: &mut DocxContext<'_>,
) -> Result<Vec<Block>, ConversionError> {
    let mut rows = Vec::new();
    let mut trailing_images = Vec::new();
    for row in table.children().filter(|node| node.is(W_NS, "tr")) {
        let mut cells = Vec::new();
        for cell in row.children().filter(|node| node.is(W_NS, "tc")) {
            let mut content = Vec::new();
            for paragraph in cell.children().filter(|node| node.is(W_NS, "p")) {
                if !content.is_empty() {
                    content.push(Inline::LineBreak);
                }
                for child in paragraph.children().filter(|child| !child.is(W_NS, "pPr")) {
                    append_inline_content(child, context, &mut content, &mut trailing_images)?;
                }
            }
            cells.push(content);
        }
        rows.push(cells);
    }
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut blocks = if rows.is_empty() {
        Vec::new()
    } else {
        vec![Block::Table {
            alignments: vec![Alignment::None; columns],
            rows,
        }]
    };
    blocks.extend(trailing_images.into_iter().map(|asset_id| Block::Image {
        asset_id,
        alt: String::new(),
    }));
    Ok(blocks)
}

fn parse_styles(archive: &Archive) -> Result<HashMap<String, u8>, ConversionError> {
    const MAX_STYLES: u64 = 4_096;
    const MAX_INHERITANCE_DEPTH: u64 = 128;
    const MAX_RESOLUTION_WORK: u64 = MAX_STYLES * MAX_INHERITANCE_DEPTH;
    let Some(entry) = archive.optional("word/styles.xml") else {
        return Ok(HashMap::new());
    };
    let parsed = parse_xml_bytes(&entry.data, "word/styles.xml")?;
    if !parsed.roots[0].is(W_NS, "styles") {
        return Err(corrupt_error(
            "DOCX styles root has the wrong expanded name",
        ));
    }
    struct StyleDefinition {
        level: Option<u8>,
        based_on: Option<String>,
    }

    let mut definitions = HashMap::new();
    for style in parsed.roots[0].descendants_ns(W_NS, "style") {
        let Some(id) = style.attr_ns(Some(W_NS), "styleId") else {
            continue;
        };
        let outline = style
            .descendants_ns(W_NS, "outlineLvl")
            .next()
            .and_then(|node| node.attr_ns(Some(W_NS), "val"))
            .and_then(|value| value.parse::<u8>().ok())
            .and_then(|value| value.checked_add(1));
        let named = style
            .child_ns(W_NS, "name")
            .and_then(|node| node.attr_ns(Some(W_NS), "val"))
            .or(Some(id))
            .and_then(heading_from_name);
        definitions.insert(
            id.to_owned(),
            StyleDefinition {
                level: outline.or(named).filter(|level| *level <= 6),
                based_on: style
                    .child_ns(W_NS, "basedOn")
                    .and_then(|node| node.attr_ns(Some(W_NS), "val"))
                    .map(ToOwned::to_owned),
            },
        );
        let count = u64::try_from(definitions.len()).unwrap_or(u64::MAX);
        if count > MAX_STYLES {
            return Err(ConversionError::LimitExceeded {
                limit: "docx_styles",
                actual: count,
                maximum: MAX_STYLES,
            });
        }
    }

    let mut resolved = HashMap::<String, (Option<u8>, u64)>::new();
    let mut work = 0u64;
    for id in definitions.keys() {
        if resolved.contains_key(id) {
            continue;
        }
        let mut path = Vec::new();
        let mut visiting = HashSet::new();
        let mut current = id.as_str();
        let (inherited, inherited_depth) = loop {
            work = work.checked_add(1).ok_or(ConversionError::LimitExceeded {
                limit: "docx_style_resolution_work",
                actual: u64::MAX,
                maximum: MAX_RESOLUTION_WORK,
            })?;
            if work > MAX_RESOLUTION_WORK {
                return Err(ConversionError::LimitExceeded {
                    limit: "docx_style_resolution_work",
                    actual: work,
                    maximum: MAX_RESOLUTION_WORK,
                });
            }
            if let Some(value) = resolved.get(current) {
                break *value;
            }
            if !visiting.insert(current.to_owned()) {
                return Err(corrupt_error(format!(
                    "DOCX style inheritance cycle includes {current:?}"
                )));
            }
            path.push(current.to_owned());
            let depth = u64::try_from(path.len()).unwrap_or(u64::MAX);
            if depth > MAX_INHERITANCE_DEPTH {
                return Err(ConversionError::LimitExceeded {
                    limit: "docx_style_inheritance_depth",
                    actual: depth,
                    maximum: MAX_INHERITANCE_DEPTH,
                });
            }
            let Some(definition) = definitions.get(current) else {
                break (None, 0);
            };
            if let Some(level) = definition.level {
                break (Some(level), 0);
            }
            let Some(parent) = definition.based_on.as_deref() else {
                break (None, 0);
            };
            current = parent;
        };
        let total_depth =
            inherited_depth.saturating_add(u64::try_from(path.len()).unwrap_or(u64::MAX));
        if total_depth > MAX_INHERITANCE_DEPTH {
            return Err(ConversionError::LimitExceeded {
                limit: "docx_style_inheritance_depth",
                actual: total_depth,
                maximum: MAX_INHERITANCE_DEPTH,
            });
        }
        let mut depth = inherited_depth;
        for style in path.into_iter().rev() {
            depth = depth.saturating_add(1);
            resolved.insert(style, (inherited, depth));
        }
    }
    Ok(resolved
        .into_iter()
        .filter_map(|(id, (level, _))| level.map(|level| (id, level)))
        .collect())
}

fn heading_from_name(name: &str) -> Option<u8> {
    let compact = name.to_ascii_lowercase().replace([' ', '-'], "");
    compact
        .strip_prefix("heading")
        .or_else(|| compact.strip_prefix("title"))
        .and_then(|value| value.parse().ok())
}

fn parse_numbering(archive: &Archive) -> Result<Numbering, ConversionError> {
    let Some(entry) = archive.optional("word/numbering.xml") else {
        return Ok(HashMap::new());
    };
    let parsed = parse_xml_bytes(&entry.data, "word/numbering.xml")?;
    let root = &parsed.roots[0];
    if !root.is(W_NS, "numbering") {
        return Err(corrupt_error(
            "DOCX numbering root has the wrong expanded name",
        ));
    }
    let mut abstracts = HashMap::<String, HashMap<u32, (bool, u64)>>::new();
    for abstract_num in root.children().filter(|node| node.is(W_NS, "abstractNum")) {
        let Some(id) = abstract_num.attr_ns(Some(W_NS), "abstractNumId") else {
            continue;
        };
        let mut levels = HashMap::new();
        for level in abstract_num.children().filter(|node| node.is(W_NS, "lvl")) {
            let index = level
                .attr_ns(Some(W_NS), "ilvl")
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            let format = level
                .child_ns(W_NS, "numFmt")
                .and_then(|node| node.attr_ns(Some(W_NS), "val"))
                .unwrap_or("bullet");
            let start = level
                .child_ns(W_NS, "start")
                .and_then(|node| node.attr_ns(Some(W_NS), "val"))
                .and_then(|value| value.parse().ok())
                .unwrap_or(1);
            levels.insert(index, (format != "bullet", start));
        }
        abstracts.insert(id.to_owned(), levels);
    }
    let mut output = HashMap::new();
    for num in root.children().filter(|node| node.is(W_NS, "num")) {
        let Some(id) = num.attr_ns(Some(W_NS), "numId") else {
            continue;
        };
        let Some(abstract_id) = num
            .child_ns(W_NS, "abstractNumId")
            .and_then(|node| node.attr_ns(Some(W_NS), "val"))
        else {
            continue;
        };
        if let Some(levels) = abstracts.get(abstract_id) {
            for (level, kind) in levels {
                output.insert((id.to_owned(), *level), *kind);
            }
        }
    }
    Ok(output)
}

fn core_metadata(archive: &Archive) -> Result<DocumentMetadata, ConversionError> {
    let Some(entry) = archive.optional("docProps/core.xml") else {
        return Ok(DocumentMetadata::default());
    };
    let parsed = parse_xml_bytes(&entry.data, "docProps/core.xml")?;
    let root = &parsed.roots[0];
    if !root.is(CP_NS, "coreProperties") {
        return Err(corrupt_error(
            "DOCX core properties root has the wrong expanded name",
        ));
    }
    let value = |name| {
        root.descendants_ns(DC_NS, name)
            .next()
            .map(XmlNode::text)
            .filter(|text| !text.trim().is_empty())
    };
    Ok(DocumentMetadata {
        title: value("title"),
        author: value("creator"),
        subject: value("subject"),
        properties: BTreeMap::new(),
        ..DocumentMetadata::default()
    })
}

fn corrupt_error(message: impl Into<String>) -> ConversionError {
    ConversionError::CorruptInput {
        message: message.into(),
    }
}
