use std::collections::BTreeMap;

use mdconvert_core::{
    Block, ConversionError, ConversionRequest, ConversionWarning, Converter, Document,
    DocumentMetadata, Inline, ListItem, WarningCode,
};
use quick_xml::{
    Reader,
    escape::unescape,
    events::{BytesStart, Event},
};

use crate::{StructuredFormat, ensure_format, read_input, strip_utf8_bom, utf8};

const MAX_XML_DEPTH: usize = 128;

#[derive(Debug, Default, Clone, Copy)]
pub struct XmlConverter;

impl Converter for XmlConverter {
    fn convert(&self, request: &ConversionRequest) -> Result<Document, ConversionError> {
        let bytes = read_input(request)?;
        ensure_format(request, &bytes, StructuredFormat::Xml)?;
        let input = utf8(strip_utf8_bom(&bytes), &request.source)?;
        let parsed = parse_xml(input)?;
        let mut properties = BTreeMap::new();
        properties.insert("attribute_order".into(), "source".into());
        properties.insert("namespace_policy".into(), "qualified_names".into());
        Ok(Document {
            metadata: DocumentMetadata {
                source_format: Some("xml".into()),
                properties,
                ..DocumentMetadata::default()
            },
            blocks: vec![element_list(&parsed.roots)],
            assets: Vec::new(),
            warnings: parsed.warnings,
        })
    }
}

#[derive(Debug)]
struct XmlNode {
    name: String,
    attributes: Vec<(String, String)>,
    content: Vec<XmlContent>,
}

#[derive(Debug)]
enum XmlContent {
    Text(String),
    Element(XmlNode),
}

struct ParsedXml {
    roots: Vec<XmlNode>,
    warnings: Vec<ConversionWarning>,
}

fn parse_xml(input: &str) -> Result<ParsedXml, ConversionError> {
    let mut reader = Reader::from_str(input);
    reader.config_mut().check_end_names = true;
    let mut stack = Vec::new();
    let mut roots = Vec::new();
    let mut outside_text = String::new();
    loop {
        let event = reader.read_event().map_err(xml_error)?;
        match event {
            Event::Start(start) => {
                check_depth(stack.len() + 1)?;
                stack.push(node_from_start(&reader, &start)?);
            }
            Event::Empty(start) => {
                check_depth(stack.len() + 1)?;
                append_node(node_from_start(&reader, &start)?, &mut stack, &mut roots);
            }
            Event::End(_) => {
                let node = stack.pop().ok_or_else(|| ConversionError::CorruptInput {
                    message: "XML closing element has no matching start element".into(),
                })?;
                append_node(node, &mut stack, &mut roots);
            }
            Event::Text(text) => {
                let value = text
                    .xml_content()
                    .map_err(|error| ConversionError::CorruptInput {
                        message: format!("invalid XML text encoding: {error}"),
                    })?;
                append_text(value.into_owned(), &mut stack, &mut outside_text);
            }
            Event::CData(text) => {
                let value = text
                    .xml_content()
                    .map_err(|error| ConversionError::CorruptInput {
                        message: format!("invalid XML CDATA encoding: {error}"),
                    })?;
                append_text(value.into_owned(), &mut stack, &mut outside_text);
            }
            Event::GeneralRef(reference) => {
                let name =
                    reference
                        .xml_content()
                        .map_err(|error| ConversionError::CorruptInput {
                            message: format!("invalid XML entity encoding: {error}"),
                        })?;
                let encoded = format!("&{name};");
                let value = unescape(&encoded).map_err(|error| ConversionError::CorruptInput {
                    message: format!("unsafe or unknown XML entity {encoded:?}: {error}"),
                })?;
                append_text(value.into_owned(), &mut stack, &mut outside_text);
            }
            Event::DocType(_) => {
                return Err(ConversionError::CorruptInput {
                    message: "XML document types and entity declarations are not allowed".into(),
                });
            }
            Event::Decl(_) | Event::PI(_) | Event::Comment(_) => {}
            Event::Eof => break,
        }
    }
    if !stack.is_empty() {
        return Err(ConversionError::CorruptInput {
            message: "XML input ended before all elements were closed".into(),
        });
    }
    if !outside_text.trim().is_empty() {
        return Err(ConversionError::CorruptInput {
            message: "non-whitespace content exists outside the XML root element".into(),
        });
    }
    if roots.len() != 1 {
        return Err(ConversionError::CorruptInput {
            message: format!(
                "XML must have exactly one root element, found {}",
                roots.len()
            ),
        });
    }
    let trimmed = trim_xml_text(&mut roots);
    let warnings = trimmed
        .then(|| ConversionWarning {
            code: WarningCode::TableDegraded,
            message: "XML formatting whitespace around text was trimmed for Markdown output".into(),
            page: None,
        })
        .into_iter()
        .collect();
    Ok(ParsedXml { roots, warnings })
}

fn node_from_start(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
) -> Result<XmlNode, ConversionError> {
    let name = std::str::from_utf8(start.name().as_ref())
        .map_err(|error| ConversionError::CorruptInput {
            message: format!("XML element name is not UTF-8: {error}"),
        })?
        .to_owned();
    let mut attributes = Vec::new();
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|error| ConversionError::CorruptInput {
            message: format!("invalid XML attribute: {error}"),
        })?;
        let key = std::str::from_utf8(attribute.key.as_ref())
            .map_err(|error| ConversionError::CorruptInput {
                message: format!("XML attribute name is not UTF-8: {error}"),
            })?
            .to_owned();
        let value = attribute
            .decode_and_unescape_value(reader.decoder())
            .map_err(|error| ConversionError::CorruptInput {
                message: format!("invalid or unsafe XML attribute value: {error}"),
            })?
            .into_owned();
        attributes.push((key, value));
    }
    Ok(XmlNode {
        name,
        attributes,
        content: Vec::new(),
    })
}

fn append_node(node: XmlNode, stack: &mut [XmlNode], roots: &mut Vec<XmlNode>) {
    if let Some(parent) = stack.last_mut() {
        parent.content.push(XmlContent::Element(node));
    } else {
        roots.push(node);
    }
}

fn append_text(text: String, stack: &mut [XmlNode], outside: &mut String) {
    let Some(parent) = stack.last_mut() else {
        outside.push_str(&text);
        return;
    };
    if let Some(XmlContent::Text(existing)) = parent.content.last_mut() {
        existing.push_str(&text);
    } else {
        parent.content.push(XmlContent::Text(text));
    }
}

fn check_depth(actual: usize) -> Result<(), ConversionError> {
    if actual > MAX_XML_DEPTH {
        return Err(ConversionError::LimitExceeded {
            limit: "xml_nesting_depth",
            actual: u64::try_from(actual).unwrap_or(u64::MAX),
            maximum: u64::try_from(MAX_XML_DEPTH).unwrap_or(u64::MAX),
        });
    }
    Ok(())
}

fn trim_xml_text(nodes: &mut [XmlNode]) -> bool {
    let mut changed = false;
    for node in nodes {
        for content in &mut node.content {
            match content {
                XmlContent::Text(text) => {
                    let trimmed = text.trim();
                    if trimmed != text {
                        changed |= !text.is_empty();
                        *text = trimmed.to_owned();
                    }
                }
                XmlContent::Element(child) => changed |= trim_xml_text(std::slice::from_mut(child)),
            }
        }
        node.content
            .retain(|content| !matches!(content, XmlContent::Text(text) if text.is_empty()));
    }
    changed
}

fn element_list(nodes: &[XmlNode]) -> Block {
    Block::List {
        ordered: false,
        start: None,
        items: nodes.iter().map(element_item).collect(),
    }
}

fn element_item(node: &XmlNode) -> ListItem {
    let mut heading = vec![Inline::Strong(vec![Inline::Text(node.name.clone())])];
    if !node.attributes.is_empty() {
        heading.push(Inline::Text(" (".into()));
        for (index, (key, value)) in node.attributes.iter().enumerate() {
            if index > 0 {
                heading.push(Inline::Text(", ".into()));
            }
            heading.push(Inline::Strong(vec![Inline::Text(key.clone())]));
            heading.push(Inline::Text(": ".into()));
            heading.push(Inline::Code(value.clone()));
        }
        heading.push(Inline::Text(")".into()));
    }
    if let [XmlContent::Text(text)] = node.content.as_slice() {
        heading.push(Inline::Text(": ".into()));
        heading.extend(text_inlines(text));
        return ListItem {
            blocks: vec![Block::Paragraph { content: heading }],
        };
    }

    let mut blocks = vec![Block::Paragraph { content: heading }];
    for content in &node.content {
        match content {
            XmlContent::Text(text) => blocks.push(Block::Paragraph {
                content: text_inlines(text),
            }),
            XmlContent::Element(child) => {
                blocks.push(element_list(std::slice::from_ref(child)));
            }
        }
    }
    ListItem { blocks }
}

fn text_inlines(text: &str) -> Vec<Inline> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut inlines = Vec::new();
    for (index, line) in normalized.split('\n').enumerate() {
        if index > 0 {
            inlines.push(Inline::LineBreak);
        }
        if !line.is_empty() {
            inlines.push(Inline::Text(line.to_owned()));
        }
    }
    inlines
}

fn xml_error(error: quick_xml::Error) -> ConversionError {
    ConversionError::CorruptInput {
        message: format!("invalid XML: {error}"),
    }
}
