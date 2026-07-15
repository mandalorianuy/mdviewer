use std::collections::{BTreeMap, HashMap, HashSet};

use mdconvert_core::{
    Alignment, Block, ConversionError, ConversionRequest, ConversionWarning, Converter, Document,
    DocumentMetadata, Inline, ListItem, WarningCode,
};

use crate::{
    archive::{
        Archive, ArchiveLimits, AssetSink, Relationship, parse_xml_bytes, relationships,
        resolve_package_path,
    },
    xml::XmlNode,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct DocxConverter;

impl Converter for DocxConverter {
    fn convert(&self, request: &ConversionRequest) -> Result<Document, ConversionError> {
        let archive = Archive::open(request, &ArchiveLimits::default())?;
        let document = parse_xml_bytes(
            &archive.entry("word/document.xml")?.data,
            "word/document.xml",
        )?;
        let root = document
            .roots
            .first()
            .ok_or_else(|| corrupt_error("DOCX document XML is empty"))?;
        let body = root
            .child("body")
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
            assets: &mut assets,
            warnings: &mut warnings,
        };
        let mut blocks = Vec::new();
        let children: Vec<_> = body.children().collect();
        let mut index = 0usize;
        while index < children.len() {
            match children[index].local_name() {
                "p" => {
                    let paragraph =
                        convert_paragraph(children[index], &styles, &numbering, &mut context)?;
                    if paragraph.list.is_some() {
                        let mut paragraphs = Vec::new();
                        while index < children.len() && children[index].local_name() == "p" {
                            let candidate = convert_paragraph(
                                children[index],
                                &styles,
                                &numbering,
                                &mut context,
                            )?;
                            if candidate.list.is_none() {
                                break;
                            }
                            paragraphs.push(candidate);
                            index += 1;
                        }
                        let mut cursor = 0usize;
                        blocks.push(build_list(
                            &paragraphs,
                            &mut cursor,
                            paragraphs[0].list.unwrap().level,
                        ));
                        continue;
                    }
                    blocks.extend(paragraph.into_blocks());
                }
                "tbl" => blocks.extend(convert_table(children[index], &mut context)?),
                _ => {}
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
    let properties = node.child("pPr");
    let heading = properties
        .and_then(|value| value.child("pStyle"))
        .and_then(|value| value.attr("val"))
        .and_then(|style| styles.get(style).copied())
        .or_else(|| {
            properties
                .and_then(|value| value.child("outlineLvl"))
                .and_then(|value| value.attr("val"))
                .and_then(|value| value.parse::<u8>().ok())
                .and_then(|value| value.checked_add(1))
                .filter(|value| *value <= 6)
        });
    let list = properties
        .and_then(|value| value.child("numPr"))
        .and_then(|numbering_properties| {
            let level = numbering_properties
                .child("ilvl")?
                .attr("val")?
                .parse::<u32>()
                .ok()?;
            let id = numbering_properties.child("numId")?.attr("val")?;
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
    for child in node.children().filter(|child| child.local_name() != "pPr") {
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
    match node.local_name() {
        "r" => {
            let mut run = Vec::new();
            for child in node.children().filter(|child| child.local_name() != "rPr") {
                match child.local_name() {
                    "t" | "delText" | "instrText" => run.push(Inline::Text(child.text())),
                    "tab" => run.push(Inline::Text("\t".into())),
                    "br" | "cr" => run.push(Inline::LineBreak),
                    "drawing" | "pict" => {
                        for blip in child.descendants("blip") {
                            if let Some(id) = blip.attr_prefixed("embed") {
                                images.push(add_related_image(id, context)?);
                            }
                        }
                    }
                    _ => append_inline_content(child, context, &mut run, images)?,
                }
            }
            let properties = node.child("rPr");
            if properties.is_some_and(|value| value.child("b").is_some()) {
                run = vec![Inline::Strong(run)];
            }
            if properties.is_some_and(|value| value.child("i").is_some()) {
                run = vec![Inline::Emphasis(run)];
            }
            output.extend(run);
        }
        "hyperlink" => {
            let mut label = Vec::new();
            for child in node.children() {
                append_inline_content(child, context, &mut label, images)?;
            }
            let anchor = node.attr("anchor").map(|value| format!("#{value}"));
            let relationship = node.attr_prefixed("id").and_then(|id| context.rels.get(id));
            let target = anchor.or_else(|| relationship.map(|value| value.target.clone()));
            if relationship.is_some_and(|value| value.external) {
                context.warnings.push(ConversionWarning {
                    code: WarningCode::ExternalAssetSkipped,
                    message: "External DOCX hyperlink relationship was preserved as text".into(),
                    page: None,
                });
                output.extend(label);
            } else if let Some(target) = target.as_ref().filter(|target| safe_link(target)) {
                output.push(Inline::Link {
                    url: target.clone(),
                    title: None,
                    content: label,
                });
            } else {
                if target.is_some() {
                    context.warnings.push(ConversionWarning {
                        code: WarningCode::InvalidLinkSkipped,
                        message: "Unsafe DOCX hyperlink was preserved as text".into(),
                        page: None,
                    });
                }
                output.extend(label);
            }
        }
        _ => {
            for child in node.children() {
                append_inline_content(child, context, output, images)?;
            }
        }
    }
    Ok(())
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
    let path = resolve_package_path("word/document.xml", &relationship.target)?;
    context.assets.add(context.archive, &path, context.request)
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
    for row in table.children().filter(|node| node.local_name() == "tr") {
        let mut cells = Vec::new();
        for cell in row.children().filter(|node| node.local_name() == "tc") {
            let mut content = Vec::new();
            for paragraph in cell.children().filter(|node| node.local_name() == "p") {
                if !content.is_empty() {
                    content.push(Inline::LineBreak);
                }
                for child in paragraph
                    .children()
                    .filter(|child| child.local_name() != "pPr")
                {
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
    let Some(entry) = archive.optional("word/styles.xml") else {
        return Ok(HashMap::new());
    };
    let parsed = parse_xml_bytes(&entry.data, "word/styles.xml")?;
    struct StyleDefinition {
        level: Option<u8>,
        based_on: Option<String>,
    }

    let mut definitions = HashMap::new();
    for style in parsed.roots[0].descendants("style") {
        let Some(id) = style.attr("styleId") else {
            continue;
        };
        let outline = style
            .descendants("outlineLvl")
            .next()
            .and_then(|node| node.attr("val"))
            .and_then(|value| value.parse::<u8>().ok())
            .and_then(|value| value.checked_add(1));
        let named = style
            .child("name")
            .and_then(|node| node.attr("val"))
            .or(Some(id))
            .and_then(heading_from_name);
        definitions.insert(
            id.to_owned(),
            StyleDefinition {
                level: outline.or(named).filter(|level| *level <= 6),
                based_on: style
                    .child("basedOn")
                    .and_then(|node| node.attr("val"))
                    .map(ToOwned::to_owned),
            },
        );
    }

    fn resolve(
        id: &str,
        definitions: &HashMap<String, StyleDefinition>,
        visiting: &mut HashSet<String>,
    ) -> Result<Option<u8>, ConversionError> {
        if !visiting.insert(id.to_owned()) {
            return Err(corrupt_error(format!(
                "DOCX style inheritance cycle includes {id:?}"
            )));
        }
        let level = if let Some(definition) = definitions.get(id) {
            if definition.level.is_some() {
                definition.level
            } else if let Some(parent) = &definition.based_on {
                resolve(parent, definitions, visiting)?
            } else {
                None
            }
        } else {
            None
        };
        visiting.remove(id);
        Ok(level)
    }

    let mut output = HashMap::new();
    for id in definitions.keys() {
        if let Some(level) = resolve(id, &definitions, &mut HashSet::new())? {
            output.insert(id.clone(), level);
        }
    }
    Ok(output)
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
    let mut abstracts = HashMap::<String, HashMap<u32, (bool, u64)>>::new();
    for abstract_num in root
        .children()
        .filter(|node| node.local_name() == "abstractNum")
    {
        let Some(id) = abstract_num.attr("abstractNumId") else {
            continue;
        };
        let mut levels = HashMap::new();
        for level in abstract_num
            .children()
            .filter(|node| node.local_name() == "lvl")
        {
            let index = level
                .attr("ilvl")
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            let format = level
                .child("numFmt")
                .and_then(|node| node.attr("val"))
                .unwrap_or("bullet");
            let start = level
                .child("start")
                .and_then(|node| node.attr("val"))
                .and_then(|value| value.parse().ok())
                .unwrap_or(1);
            levels.insert(index, (format != "bullet", start));
        }
        abstracts.insert(id.to_owned(), levels);
    }
    let mut output = HashMap::new();
    for num in root.children().filter(|node| node.local_name() == "num") {
        let Some(id) = num.attr("numId") else {
            continue;
        };
        let Some(abstract_id) = num.child("abstractNumId").and_then(|node| node.attr("val")) else {
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
    let value = |name| {
        root.descendants(name)
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
