use std::collections::BTreeMap;

use mdconvert_core::{
    Block, ConversionError, ConversionRequest, Converter, Document, DocumentMetadata, Inline,
    ListItem,
};
use quick_xml::{
    Reader, XmlVersion,
    escape::unescape,
    events::{BytesDecl, BytesPI, BytesStart, Event},
};

use crate::{
    StructuredFormat, StructuredLimits, ensure_format, limit_exceeded, read_input, strip_utf8_bom,
    utf8,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct XmlConverter;

impl XmlConverter {
    pub fn convert_with_limits(
        &self,
        request: &ConversionRequest,
        limits: &StructuredLimits,
    ) -> Result<Document, ConversionError> {
        limits.validate()?;
        let bytes = read_input(request)?;
        ensure_format(request, &bytes, StructuredFormat::Xml, limits)?;
        let input = utf8(strip_utf8_bom(&bytes), &request.source)?;
        let parsed = parse_xml(input, limits)?;
        let mut properties = BTreeMap::new();
        properties.insert("attribute_order".into(), "source".into());
        properties.insert("namespace_policy".into(), "qualified_names".into());
        properties.insert(
            "indentation_policy".into(),
            "discard_newline_whitespace_between_child_elements".into(),
        );
        Ok(Document {
            metadata: DocumentMetadata {
                source_format: Some("xml".into()),
                properties,
                ..DocumentMetadata::default()
            },
            blocks: vec![element_list(&parsed.roots)],
            assets: Vec::new(),
            warnings: Vec::new(),
        })
    }
}

impl Converter for XmlConverter {
    fn convert(&self, request: &ConversionRequest) -> Result<Document, ConversionError> {
        self.convert_with_limits(request, &StructuredLimits::default())
    }
}

pub(crate) fn validate_xml_candidate(
    input: &str,
    limits: &StructuredLimits,
) -> Result<bool, ConversionError> {
    match parse_xml(input, limits) {
        Ok(_) => Ok(true),
        Err(error @ ConversionError::LimitExceeded { .. }) => Err(error),
        Err(_) => Ok(false),
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DocumentPhase {
    Prolog,
    Root,
    Epilog,
}

struct XmlBudget<'a> {
    limits: &'a StructuredLimits,
    nodes: u64,
    attributes: u64,
    text_bytes: u64,
}

impl<'a> XmlBudget<'a> {
    fn new(limits: &'a StructuredLimits) -> Self {
        Self {
            limits,
            nodes: 0,
            attributes: 0,
            text_bytes: 0,
        }
    }

    fn add_node(&mut self) -> Result<(), ConversionError> {
        let actual = match self.nodes.checked_add(1) {
            Some(actual) => actual,
            None => {
                return Err(limit_exceeded(
                    "xml_nodes",
                    u64::MAX,
                    self.limits.max_xml_nodes,
                ));
            }
        };
        if actual > self.limits.max_xml_nodes {
            return Err(limit_exceeded(
                "xml_nodes",
                actual,
                self.limits.max_xml_nodes,
            ));
        }
        self.nodes = actual;
        Ok(())
    }

    fn add_attribute(&mut self, element_count: u64) -> Result<(), ConversionError> {
        if element_count > self.limits.max_xml_attributes_per_element {
            return Err(limit_exceeded(
                "xml_attributes_per_element",
                element_count,
                self.limits.max_xml_attributes_per_element,
            ));
        }
        let actual = match self.attributes.checked_add(1) {
            Some(actual) => actual,
            None => {
                return Err(limit_exceeded(
                    "xml_attributes",
                    u64::MAX,
                    self.limits.max_xml_attributes,
                ));
            }
        };
        if actual > self.limits.max_xml_attributes {
            return Err(limit_exceeded(
                "xml_attributes",
                actual,
                self.limits.max_xml_attributes,
            ));
        }
        self.attributes = actual;
        Ok(())
    }

    fn add_text(&mut self, bytes: usize) -> Result<(), ConversionError> {
        let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
        let actual = match self.text_bytes.checked_add(bytes) {
            Some(actual) => actual,
            None => {
                return Err(limit_exceeded(
                    "xml_text_bytes",
                    u64::MAX,
                    self.limits.max_xml_text_bytes,
                ));
            }
        };
        if actual > self.limits.max_xml_text_bytes {
            return Err(limit_exceeded(
                "xml_text_bytes",
                actual,
                self.limits.max_xml_text_bytes,
            ));
        }
        self.text_bytes = actual;
        Ok(())
    }
}

fn parse_xml(input: &str, limits: &StructuredLimits) -> Result<ParsedXml, ConversionError> {
    let mut reader = Reader::from_str(input);
    reader.config_mut().enable_all_checks(true);
    reader.config_mut().allow_dangling_amp = false;
    reader.config_mut().allow_unmatched_ends = false;
    let mut budget = XmlBudget::new(limits);
    let mut stack = Vec::new();
    let mut roots = Vec::new();
    let mut phase = DocumentPhase::Prolog;
    let mut declaration_seen = false;
    let mut prolog_event_seen = false;

    loop {
        let event = reader.read_event().map_err(xml_error)?;
        match event {
            Event::Decl(declaration) => {
                if phase != DocumentPhase::Prolog || declaration_seen || prolog_event_seen {
                    return corrupt(
                        "XML declaration must appear exactly once at the start of the prolog",
                    );
                }
                validate_declaration(&declaration)?;
                declaration_seen = true;
                prolog_event_seen = true;
            }
            Event::Start(start) => {
                if phase == DocumentPhase::Epilog {
                    return corrupt("XML epilog cannot contain another root element");
                }
                if phase == DocumentPhase::Prolog {
                    phase = DocumentPhase::Root;
                    prolog_event_seen = true;
                }
                check_depth(stack.len() + 1, limits)?;
                budget.add_node()?;
                stack.push(node_from_start(&reader, &start, &mut budget)?);
            }
            Event::Empty(start) => {
                if phase == DocumentPhase::Epilog {
                    return corrupt("XML epilog cannot contain another root element");
                }
                if phase == DocumentPhase::Prolog {
                    phase = DocumentPhase::Root;
                    prolog_event_seen = true;
                }
                check_depth(stack.len() + 1, limits)?;
                budget.add_node()?;
                append_node(
                    node_from_start(&reader, &start, &mut budget)?,
                    &mut stack,
                    &mut roots,
                );
                if stack.is_empty() {
                    phase = DocumentPhase::Epilog;
                }
            }
            Event::End(_) => {
                if phase != DocumentPhase::Root {
                    return corrupt("XML closing element is outside the root element");
                }
                let node = stack.pop().ok_or_else(|| ConversionError::CorruptInput {
                    message: "XML closing element has no matching start element".into(),
                })?;
                append_node(node, &mut stack, &mut roots);
                if stack.is_empty() {
                    phase = DocumentPhase::Epilog;
                }
            }
            Event::Text(text) => {
                let value = text
                    .xml_content(XmlVersion::Implicit1_0)
                    .map_err(|error| ConversionError::CorruptInput {
                        message: format!("invalid XML text encoding: {error}"),
                    })?
                    .into_owned();
                if phase == DocumentPhase::Root && !stack.is_empty() {
                    budget.add_text(value.len())?;
                    append_text(value, &mut stack);
                } else {
                    if !value.trim().is_empty() {
                        return corrupt("only whitespace is allowed outside the XML root element");
                    }
                    if phase == DocumentPhase::Prolog && !value.is_empty() {
                        prolog_event_seen = true;
                    }
                }
            }
            Event::CData(text) => {
                if phase != DocumentPhase::Root || stack.is_empty() {
                    return corrupt("CDATA is not allowed outside the XML root element");
                }
                let value = text
                    .xml_content(XmlVersion::Implicit1_0)
                    .map_err(|error| ConversionError::CorruptInput {
                        message: format!("invalid XML CDATA encoding: {error}"),
                    })?
                    .into_owned();
                budget.add_text(value.len())?;
                append_text(value, &mut stack);
            }
            Event::GeneralRef(reference) => {
                if phase != DocumentPhase::Root || stack.is_empty() {
                    return corrupt(
                        "entity references are not allowed outside the XML root element",
                    );
                }
                let name = reference
                    .xml_content(XmlVersion::Implicit1_0)
                    .map_err(|error| ConversionError::CorruptInput {
                        message: format!("invalid XML entity encoding: {error}"),
                    })?;
                let encoded = format!("&{name};");
                let value = unescape(&encoded).map_err(|error| ConversionError::CorruptInput {
                    message: format!("unsafe or unknown XML entity {encoded:?}: {error}"),
                })?;
                budget.add_text(value.len())?;
                append_text(value.into_owned(), &mut stack);
            }
            Event::DocType(_) => {
                return corrupt("XML document types and entity declarations are not allowed");
            }
            Event::PI(instruction) => {
                validate_processing_instruction(&instruction)?;
                if phase == DocumentPhase::Prolog {
                    prolog_event_seen = true;
                }
            }
            Event::Comment(_) => {
                if phase == DocumentPhase::Prolog {
                    prolog_event_seen = true;
                }
            }
            Event::Eof => break,
        }
    }
    if !stack.is_empty() {
        return corrupt("XML input ended before all elements were closed");
    }
    if roots.len() != 1 || phase != DocumentPhase::Epilog {
        return Err(ConversionError::CorruptInput {
            message: format!(
                "XML must have exactly one root element, found {}",
                roots.len()
            ),
        });
    }
    discard_indentation(&mut roots);
    Ok(ParsedXml { roots })
}

fn validate_declaration(declaration: &BytesDecl<'_>) -> Result<(), ConversionError> {
    let version = declaration.version().map_err(xml_error)?;
    if version.as_ref() != b"1.0" {
        return corrupt("only XML version 1.0 is supported");
    }
    if let Some(encoding) = declaration.encoding() {
        let encoding = encoding.map_err(|error| ConversionError::CorruptInput {
            message: format!("invalid XML encoding declaration: {error}"),
        })?;
        if !encoding.as_ref().eq_ignore_ascii_case(b"UTF-8") {
            return corrupt("XML declaration encoding must be UTF-8");
        }
    }
    if let Some(standalone) = declaration.standalone() {
        let standalone = standalone.map_err(|error| ConversionError::CorruptInput {
            message: format!("invalid XML standalone declaration: {error}"),
        })?;
        if standalone.as_ref() != b"yes" && standalone.as_ref() != b"no" {
            return corrupt("XML standalone declaration must be yes or no");
        }
    }
    Ok(())
}

fn validate_processing_instruction(instruction: &BytesPI<'_>) -> Result<(), ConversionError> {
    if instruction.target().eq_ignore_ascii_case(b"xml") {
        return corrupt("processing instruction target XML is reserved");
    }
    Ok(())
}

fn node_from_start(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    budget: &mut XmlBudget<'_>,
) -> Result<XmlNode, ConversionError> {
    let name = std::str::from_utf8(start.name().as_ref())
        .map_err(|error| ConversionError::CorruptInput {
            message: format!("XML element name is not UTF-8: {error}"),
        })?
        .to_owned();
    let mut attributes = Vec::new();
    let mut element_count = 0u64;
    for attribute in start.attributes() {
        element_count = match element_count.checked_add(1) {
            Some(actual) => actual,
            None => {
                return Err(limit_exceeded(
                    "xml_attributes_per_element",
                    u64::MAX,
                    budget.limits.max_xml_attributes_per_element,
                ));
            }
        };
        budget.add_attribute(element_count)?;
        let attribute = attribute.map_err(|error| ConversionError::CorruptInput {
            message: format!("invalid XML attribute: {error}"),
        })?;
        let key = std::str::from_utf8(attribute.key.as_ref())
            .map_err(|error| ConversionError::CorruptInput {
                message: format!("XML attribute name is not UTF-8: {error}"),
            })?
            .to_owned();
        let value = attribute
            .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
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

fn append_text(text: String, stack: &mut [XmlNode]) {
    let parent = stack
        .last_mut()
        .expect("document grammar ensures text has an open element");
    if let Some(XmlContent::Text(existing)) = parent.content.last_mut() {
        existing.push_str(&text);
    } else {
        parent.content.push(XmlContent::Text(text));
    }
}

fn check_depth(actual: usize, limits: &StructuredLimits) -> Result<(), ConversionError> {
    let actual = u64::try_from(actual).unwrap_or(u64::MAX);
    if actual > limits.max_xml_depth {
        return Err(limit_exceeded(
            "xml_nesting_depth",
            actual,
            limits.max_xml_depth,
        ));
    }
    Ok(())
}

fn discard_indentation(nodes: &mut [XmlNode]) {
    for node in nodes {
        for content in &mut node.content {
            if let XmlContent::Element(child) = content {
                discard_indentation(std::slice::from_mut(child));
            }
        }
        let has_child_element = node
            .content
            .iter()
            .any(|content| matches!(content, XmlContent::Element(_)));
        if has_child_element {
            node.content.retain(|content| {
                !matches!(content, XmlContent::Text(text) if text.trim().is_empty() && text.contains(['\n', '\r']))
            });
        }
    }
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

fn corrupt<T>(message: impl Into<String>) -> Result<T, ConversionError> {
    Err(ConversionError::CorruptInput {
        message: message.into(),
    })
}

fn xml_error(error: quick_xml::Error) -> ConversionError {
    ConversionError::CorruptInput {
        message: format!("invalid XML: {error}"),
    }
}
