use std::collections::HashMap;

use mdconvert_core::{
    Alignment, Block, ConversionError, ConversionRequest, ConversionWarning, Converter, Document,
    DocumentMetadata, Inline, WarningCode,
};

use crate::{
    archive::{
        Archive, ArchiveLimits, AssetSink, ContentTypes, Relationship, authenticate_ooxml,
        parse_xml_bytes, relationships, resolve_package_path,
    },
    xml::XmlNode,
};

const PPTX_MAIN_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml";
const SLIDE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide";
const IMAGE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
const NOTES_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide";
const P_NS: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const A_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

#[derive(Debug, Default, Clone, Copy)]
pub struct PptxConverter;

impl Converter for PptxConverter {
    fn convert(&self, request: &ConversionRequest) -> Result<Document, ConversionError> {
        let archive = Archive::open(request, &ArchiveLimits::default())?;
        let content_types =
            authenticate_ooxml(&archive, "ppt/presentation.xml", PPTX_MAIN_CONTENT_TYPE)?;
        let presentation = parse_xml_bytes(
            &archive.entry("ppt/presentation.xml")?.data,
            "ppt/presentation.xml",
        )?;
        let presentation_rels = relationships(&archive, "ppt/_rels/presentation.xml.rels")?
            .into_iter()
            .map(|relationship| (relationship.id.clone(), relationship))
            .collect::<HashMap<_, _>>();
        let mut slide_paths = Vec::new();
        let presentation_root = &presentation.roots[0];
        if !presentation_root.is(P_NS, "presentation") {
            return Err(corrupt_error(
                "PPTX presentation root has the wrong expanded name",
            ));
        }
        for slide in presentation_root.descendants_ns(P_NS, "sldId") {
            let id = slide
                .attr_ns(Some(R_NS), "id")
                .ok_or_else(|| corrupt_error("presentation slide is missing a relationship ID"))?;
            let relationship = presentation_rels.get(id).ok_or_else(|| {
                corrupt_error(format!(
                    "presentation slide references missing relationship {id:?}"
                ))
            })?;
            if relationship.external || relationship.kind != SLIDE_REL {
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
                u32::try_from(index + 1).unwrap_or(u32::MAX),
                request,
                &content_types,
                &mut assets,
                &mut warnings,
                &mut blocks,
            )?;
        }
        Ok(Document {
            metadata: DocumentMetadata {
                source_format: Some("pptx".into()),
                page_count: Some(u32::try_from(slide_paths.len()).unwrap_or(u32::MAX)),
                properties: [("ooxml_profile".into(), "transitional_only".into())]
                    .into_iter()
                    .collect(),
                ..DocumentMetadata::default()
            },
            blocks,
            assets: assets.into_assets(),
            warnings,
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn append_slide(
    archive: &Archive,
    slide_path: &str,
    page: u32,
    request: &ConversionRequest,
    content_types: &ContentTypes,
    assets: &mut AssetSink,
    warnings: &mut Vec<ConversionWarning>,
    blocks: &mut Vec<Block>,
) -> Result<(), ConversionError> {
    let parsed = parse_xml_bytes(&archive.entry(slide_path)?.data, slide_path)?;
    let root = &parsed.roots[0];
    if !root.is(P_NS, "sld") {
        return Err(corrupt_error("PPTX slide root has the wrong expanded name"));
    }
    let rel_path = relationship_part(slide_path)?;
    let rels = relationships(archive, &rel_path)?
        .into_iter()
        .map(|relationship| (relationship.id.clone(), relationship))
        .collect::<HashMap<_, _>>();
    let shape_tree = root.descendants_ns(P_NS, "spTree").next().unwrap_or(root);
    for node in shape_tree.children() {
        append_shape_tree_node(
            node,
            archive,
            slide_path,
            page,
            request,
            content_types,
            &rels,
            assets,
            warnings,
            blocks,
        )?;
    }
    let notes_relationships = rels
        .values()
        .filter(|relationship| relationship.kind == NOTES_REL)
        .collect::<Vec<_>>();
    if notes_relationships.len() > 1 {
        return Err(corrupt_error(
            "PPTX slide has multiple notesSlide relationships",
        ));
    }
    if let Some(notes) = notes_relationships.first() {
        if notes.external {
            return Err(corrupt_error("external PPTX notesSlide is unsupported"));
        }
        let path = resolve_package_path(slide_path, &notes.target)?;
        let notes = parse_xml_bytes(&archive.entry(&path)?.data, &path)?;
        if !notes.roots[0].is(P_NS, "notes") {
            return Err(corrupt_error("PPTX notes root has the wrong expanded name"));
        }
        let text = notes.roots[0]
            .descendants_ns(P_NS, "sp")
            .filter(|shape| {
                !shape
                    .descendants_ns(P_NS, "ph")
                    .next()
                    .and_then(|node| node.attr_ns(None, "type"))
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

#[allow(clippy::too_many_arguments)]
fn append_shape_tree_node(
    node: &XmlNode,
    archive: &Archive,
    slide_path: &str,
    page: u32,
    request: &ConversionRequest,
    content_types: &ContentTypes,
    rels: &HashMap<String, Relationship>,
    assets: &mut AssetSink,
    warnings: &mut Vec<ConversionWarning>,
    blocks: &mut Vec<Block>,
) -> Result<(), ConversionError> {
    if node.is(P_NS, "sp") {
        let content = shape_inlines(node, rels, warnings, page);
        if !content.is_empty() {
            let title = node
                .descendants_ns(P_NS, "ph")
                .next()
                .and_then(|placeholder| placeholder.attr_ns(None, "type"))
                .is_some_and(|value| matches!(value, "title" | "ctrTitle"));
            blocks.push(if title {
                Block::Heading { level: 1, content }
            } else {
                Block::Paragraph { content }
            });
        }
    } else if node.is(A_NS, "tbl") {
        if let Some(table) = convert_table(node) {
            blocks.push(table);
        }
    } else if node.is(P_NS, "graphicFrame") {
        for table in node.descendants_ns(A_NS, "tbl") {
            if let Some(table) = convert_table(table) {
                blocks.push(table);
            }
        }
    } else if node.is(A_NS, "blip") {
        append_slide_image(
            node,
            archive,
            slide_path,
            request,
            content_types,
            rels,
            assets,
            blocks,
        )?;
    } else if node.is(P_NS, "pic") {
        for blip in node.descendants_ns(A_NS, "blip") {
            append_slide_image(
                blip,
                archive,
                slide_path,
                request,
                content_types,
                rels,
                assets,
                blocks,
            )?;
        }
    } else {
        for child in node.children() {
            append_shape_tree_node(
                child,
                archive,
                slide_path,
                page,
                request,
                content_types,
                rels,
                assets,
                warnings,
                blocks,
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn append_slide_image(
    blip: &XmlNode,
    archive: &Archive,
    slide_path: &str,
    request: &ConversionRequest,
    content_types: &ContentTypes,
    rels: &HashMap<String, Relationship>,
    assets: &mut AssetSink,
    blocks: &mut Vec<Block>,
) -> Result<(), ConversionError> {
    let Some(id) = blip.attr_ns(Some(R_NS), "embed") else {
        return Ok(());
    };
    let relationship = rels.get(id).ok_or_else(|| {
        corrupt_error(format!(
            "slide image references missing relationship {id:?}"
        ))
    })?;
    if relationship.external {
        return Err(corrupt_error(format!(
            "external PPTX image relationship {id:?} is unsupported"
        )));
    }
    if relationship.kind != IMAGE_REL {
        return Err(corrupt_error(format!(
            "PPTX image relationship {id:?} has invalid type {:?}",
            relationship.kind
        )));
    }
    let path = resolve_package_path(slide_path, &relationship.target)?;
    let asset_id = assets.add(archive, &path, request, content_types)?;
    blocks.push(Block::Image {
        asset_id,
        alt: String::new(),
    });
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
        .descendants_ns(A_NS, "p")
        .map(|paragraph| {
            paragraph
                .descendants_ns(A_NS, "t")
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
    page: u32,
) -> Vec<Inline> {
    let mut output = Vec::new();
    for paragraph in shape.descendants_ns(A_NS, "p") {
        if !output.is_empty() {
            output.push(Inline::LineBreak);
        }
        for run in paragraph
            .children()
            .filter(|node| node.is(A_NS, "r") || node.is(A_NS, "fld"))
        {
            let text: String = run.descendants_ns(A_NS, "t").map(XmlNode::text).collect();
            if text.is_empty() {
                continue;
            }
            let relationship_id = run
                .descendants_ns(A_NS, "hlinkClick")
                .next()
                .and_then(|link| link.attr_ns(Some(R_NS), "id"));
            let relationship = relationship_id.and_then(|id| rels.get(id));
            if let Some(relationship) = relationship {
                if relationship.external || !safe_local_link(&relationship.target) {
                    let code = if relationship.external {
                        WarningCode::ExternalLinkSkipped
                    } else {
                        WarningCode::InvalidLinkSkipped
                    };
                    push_warning(
                        warnings,
                        code,
                        format!(
                            "PPTX hyperlink {:?} was preserved as text",
                            relationship.target
                        ),
                        Some(page),
                    );
                    output.push(Inline::Text(text));
                } else {
                    output.push(Inline::Link {
                        url: relationship.target.clone(),
                        title: None,
                        content: vec![Inline::Text(text)],
                    });
                }
            } else {
                if relationship_id.is_some() {
                    push_warning(
                        warnings,
                        WarningCode::InvalidLinkSkipped,
                        "Missing PPTX hyperlink relationship was preserved as text".into(),
                        Some(page),
                    );
                }
                output.push(Inline::Text(text));
            }
        }
    }
    output
}

fn safe_local_link(target: &str) -> bool {
    if target.starts_with('#') {
        return true;
    }
    let normalized = target.replace('\\', "/");
    !normalized.contains(':')
        && !normalized.starts_with('/')
        && !normalized.split('/').any(|part| part == "..")
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
        .filter(|row| row.is(A_NS, "tr"))
        .map(|row| {
            row.children()
                .filter(|cell| cell.is(A_NS, "tc"))
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

fn push_warning(
    warnings: &mut Vec<ConversionWarning>,
    code: WarningCode,
    message: String,
    page: Option<u32>,
) {
    if !warnings
        .iter()
        .any(|warning| warning.code == code && warning.page == page)
    {
        warnings.push(ConversionWarning {
            code,
            message,
            page,
        });
    }
}

fn corrupt_error(message: impl Into<String>) -> ConversionError {
    ConversionError::CorruptInput {
        message: message.into(),
    }
}
