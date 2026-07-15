use std::collections::HashMap;

use mdconvert_core::{
    Alignment, Block, ConversionError, ConversionRequest, ConversionWarning, Converter, Document,
    DocumentMetadata, Inline, WarningCode,
};

use crate::{
    archive::{
        Archive, ArchiveLimits, AssetSink, Relationship, parse_xml_bytes, relationships,
        resolve_package_path,
    },
    xml::XmlNode,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct PptxConverter;

impl Converter for PptxConverter {
    fn convert(&self, request: &ConversionRequest) -> Result<Document, ConversionError> {
        let archive = Archive::open(request, &ArchiveLimits::default())?;
        let presentation = parse_xml_bytes(
            &archive.entry("ppt/presentation.xml")?.data,
            "ppt/presentation.xml",
        )?;
        let presentation_rels = relationships(&archive, "ppt/_rels/presentation.xml.rels")?
            .into_iter()
            .map(|relationship| (relationship.id.clone(), relationship))
            .collect::<HashMap<_, _>>();
        let mut slide_paths = Vec::new();
        for slide in presentation.roots[0].descendants("sldId") {
            let id = slide
                .attr_prefixed("id")
                .ok_or_else(|| corrupt_error("presentation slide is missing a relationship ID"))?;
            let relationship = presentation_rels.get(id).ok_or_else(|| {
                corrupt_error(format!(
                    "presentation slide references missing relationship {id:?}"
                ))
            })?;
            if relationship.external
                || !relationship.kind.ends_with("/slide") && relationship.kind != "slide"
            {
                return Err(corrupt_error(
                    "presentation slide relationship is not local slide content",
                ));
            }
            slide_paths.push(resolve_package_path(
                "ppt/presentation.xml",
                &relationship.target,
            )?);
        }
        if slide_paths.is_empty() {
            return Err(corrupt_error(
                "PPTX presentation contains no ordered slides",
            ));
        }
        if u64::try_from(slide_paths.len()).unwrap_or(u64::MAX)
            > u64::from(request.limits.max_pages)
        {
            return Err(ConversionError::LimitExceeded {
                limit: "pages",
                actual: u64::try_from(slide_paths.len()).unwrap_or(u64::MAX),
                maximum: u64::from(request.limits.max_pages),
            });
        }

        let mut blocks = Vec::new();
        let mut warnings = Vec::new();
        let mut assets = AssetSink::new();
        for (index, slide_path) in slide_paths.iter().enumerate() {
            blocks.push(Block::Heading {
                level: 2,
                content: vec![Inline::Text(format!("Slide {}", index + 1))],
            });
            append_slide(
                &archive,
                slide_path,
                request,
                &mut assets,
                &mut warnings,
                &mut blocks,
            )?;
        }
        Ok(Document {
            metadata: DocumentMetadata {
                source_format: Some("pptx".into()),
                page_count: Some(u32::try_from(slide_paths.len()).unwrap_or(u32::MAX)),
                ..DocumentMetadata::default()
            },
            blocks,
            assets: assets.into_assets(),
            warnings,
        })
    }
}

fn append_slide(
    archive: &Archive,
    slide_path: &str,
    request: &ConversionRequest,
    assets: &mut AssetSink,
    warnings: &mut Vec<ConversionWarning>,
    blocks: &mut Vec<Block>,
) -> Result<(), ConversionError> {
    let parsed = parse_xml_bytes(&archive.entry(slide_path)?.data, slide_path)?;
    let root = &parsed.roots[0];
    let rel_path = relationship_part(slide_path)?;
    let rels = relationships(archive, &rel_path)?
        .into_iter()
        .map(|relationship| (relationship.id.clone(), relationship))
        .collect::<HashMap<_, _>>();
    for shape in root.descendants("sp") {
        let content = shape_inlines(shape, &rels, warnings);
        if content.is_empty() {
            continue;
        }
        let title = shape
            .descendants("ph")
            .next()
            .and_then(|placeholder| placeholder.attr("type"))
            .is_some_and(|value| matches!(value, "title" | "ctrTitle"));
        blocks.push(if title {
            Block::Heading { level: 1, content }
        } else {
            Block::Paragraph { content }
        });
    }
    for table in root.descendants("tbl") {
        if let Some(table) = convert_table(table) {
            blocks.push(table);
        }
    }
    for blip in root.descendants("blip") {
        let Some(id) = blip.attr_prefixed("embed") else {
            continue;
        };
        let relationship = rels.get(id).ok_or_else(|| {
            corrupt_error(format!(
                "slide image references missing relationship {id:?}"
            ))
        })?;
        if relationship.external {
            warnings.push(ConversionWarning {
                code: WarningCode::ExternalAssetSkipped,
                message: format!("External PPTX image relationship {id:?} was skipped"),
                page: None,
            });
            continue;
        }
        let path = resolve_package_path(slide_path, &relationship.target)?;
        let asset_id = assets.add(archive, &path, request)?;
        blocks.push(Block::Image {
            asset_id,
            alt: String::new(),
        });
    }
    warn_external_links(root, &rels, warnings);
    if let Some(notes) = rels.values().find(|relationship| {
        !relationship.external
            && (relationship.kind.ends_with("/notesSlide") || relationship.kind == "notesSlide")
    }) {
        let path = resolve_package_path(slide_path, &notes.target)?;
        let notes = parse_xml_bytes(&archive.entry(&path)?.data, &path)?;
        let text = notes.roots[0]
            .descendants("sp")
            .filter(|shape| {
                !shape
                    .descendants("ph")
                    .next()
                    .and_then(|node| node.attr("type"))
                    .is_some_and(|value| matches!(value, "hdr" | "ftr" | "dt" | "sldNum"))
            })
            .map(shape_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if !text.is_empty() {
            blocks.push(Block::Heading {
                level: 3,
                content: vec![Inline::Text("Notes".into())],
            });
            blocks.push(Block::Paragraph {
                content: text_lines(&text),
            });
        }
    }
    Ok(())
}

fn relationship_part(part: &str) -> Result<String, ConversionError> {
    let (directory, leaf) = part
        .rsplit_once('/')
        .ok_or_else(|| corrupt_error("package part has no parent directory"))?;
    Ok(format!("{directory}/_rels/{leaf}.rels"))
}

fn shape_text(shape: &XmlNode) -> String {
    shape
        .descendants("p")
        .map(|paragraph| {
            paragraph
                .descendants("t")
                .map(XmlNode::text)
                .collect::<String>()
        })
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn shape_inlines(
    shape: &XmlNode,
    rels: &HashMap<String, Relationship>,
    warnings: &mut Vec<ConversionWarning>,
) -> Vec<Inline> {
    let mut output = Vec::new();
    for paragraph in shape.descendants("p") {
        if !output.is_empty() {
            output.push(Inline::LineBreak);
        }
        for run in paragraph
            .children()
            .filter(|node| matches!(node.local_name(), "r" | "fld"))
        {
            let text: String = run.descendants("t").map(XmlNode::text).collect();
            if text.is_empty() {
                continue;
            }
            let relationship = run
                .descendants("hlinkClick")
                .next()
                .and_then(|link| link.attr_prefixed("id"))
                .and_then(|id| rels.get(id));
            if let Some(relationship) = relationship {
                if relationship.external || !safe_local_link(&relationship.target) {
                    warnings.push(ConversionWarning {
                        code: WarningCode::ExternalAssetSkipped,
                        message: "External or unsafe PPTX hyperlink was preserved as text".into(),
                        page: None,
                    });
                    output.push(Inline::Text(text));
                } else {
                    output.push(Inline::Link {
                        url: relationship.target.clone(),
                        title: None,
                        content: vec![Inline::Text(text)],
                    });
                }
            } else {
                output.push(Inline::Text(text));
            }
        }
    }
    output
}

fn safe_local_link(target: &str) -> bool {
    target.starts_with('#')
        || (!target.contains(':')
            && !target.starts_with('/')
            && !target
                .replace('\\', "/")
                .split('/')
                .any(|part| part == ".."))
}

fn text_lines(text: &str) -> Vec<Inline> {
    let mut output = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if index > 0 {
            output.push(Inline::LineBreak);
        }
        output.push(Inline::Text(line.into()));
    }
    output
}

fn convert_table(table: &XmlNode) -> Option<Block> {
    let rows: Vec<_> = table
        .children()
        .filter(|row| row.local_name() == "tr")
        .map(|row| {
            row.children()
                .filter(|cell| cell.local_name() == "tc")
                .map(|cell| vec![Inline::Text(shape_text(cell))])
                .collect::<Vec<_>>()
        })
        .collect();
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    (!rows.is_empty()).then(|| Block::Table {
        alignments: vec![Alignment::None; columns],
        rows,
    })
}

fn warn_external_links(
    root: &XmlNode,
    rels: &HashMap<String, Relationship>,
    warnings: &mut Vec<ConversionWarning>,
) {
    for link in root.descendants("hlinkClick") {
        if link
            .attr_prefixed("id")
            .and_then(|id| rels.get(id))
            .is_some_and(|rel| rel.external)
        {
            warnings.push(ConversionWarning {
                code: WarningCode::ExternalAssetSkipped,
                message: "External PPTX hyperlink relationship was preserved as text".into(),
                page: None,
            });
        }
    }
}

fn corrupt_error(message: impl Into<String>) -> ConversionError {
    ConversionError::CorruptInput {
        message: message.into(),
    }
}
