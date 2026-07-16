use std::collections::{BTreeMap, HashSet};

use mdconvert_core::{
    Block, ConversionError, ConversionRequest, Converter, Document, DocumentMetadata, Inline,
    ListItem,
};
use quick_xml::{
    Reader, XmlVersion,
    events::{BytesDecl, BytesPI, BytesStart, Event},
};

use crate::{
    StructuredFormat, StructuredLimits, ensure_format, limit_exceeded, read_input, strip_utf8_bom,
    utf8,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct XmlConverter;

impl XmlConverter {
    pub fn convert_bytes(
        &self,
        bytes: &[u8],
        request: &ConversionRequest,
    ) -> Result<Document, ConversionError> {
        crate::ensure_input_bytes(request, bytes)?;
        convert_xml_bytes(request, bytes, &StructuredLimits::default())
    }

    pub fn convert_with_limits(
        &self,
        request: &ConversionRequest,
        limits: &StructuredLimits,
    ) -> Result<Document, ConversionError> {
        limits.validate()?;
        let bytes = read_input(request)?;
        convert_xml_bytes(request, &bytes, limits)
    }
}

pub(crate) fn convert_xml_bytes(
    request: &ConversionRequest,
    bytes: &[u8],
    limits: &StructuredLimits,
) -> Result<Document, ConversionError> {
    limits.validate()?;
    ensure_format(request, bytes, StructuredFormat::Xml, limits)?;
    let input = utf8(strip_utf8_bom(bytes), &request.source)?;
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
pub(crate) struct XmlNode {
    pub(crate) name: String,
    pub(crate) attributes: Vec<(String, String)>,
    namespace_uri: Option<String>,
    local_name: String,
    expanded_attributes: Vec<ExpandedAttribute>,
    pub(crate) content: Vec<XmlContent>,
}

#[derive(Debug)]
struct ExpandedAttribute {
    namespace_uri: Option<String>,
    local_name: String,
    value: String,
}

impl XmlNode {
    pub(crate) fn is(&self, namespace_uri: &str, local_name: &str) -> bool {
        self.namespace_uri.as_deref() == Some(namespace_uri) && self.local_name == local_name
    }

    pub(crate) fn namespace_uri(&self) -> Option<&str> {
        self.namespace_uri.as_deref()
    }

    pub(crate) fn attr_ns(&self, namespace_uri: Option<&str>, local_name: &str) -> Option<&str> {
        self.expanded_attributes
            .iter()
            .find(|attribute| {
                attribute.namespace_uri.as_deref() == namespace_uri
                    && attribute.local_name == local_name
            })
            .map(|attribute| attribute.value.as_str())
    }

    pub(crate) fn children(&self) -> impl Iterator<Item = &XmlNode> {
        self.content.iter().filter_map(|content| match content {
            XmlContent::Element(node) => Some(node),
            XmlContent::Text(_) => None,
        })
    }

    pub(crate) fn child_ns(&self, namespace_uri: &str, local_name: &str) -> Option<&XmlNode> {
        self.children()
            .find(|node| node.is(namespace_uri, local_name))
    }

    pub(crate) fn descendants_ns<'a>(
        &'a self,
        namespace_uri: &'a str,
        local_name: &'a str,
    ) -> ExpandedDescendants<'a> {
        let mut stack: Vec<_> = self.children().collect();
        stack.reverse();
        ExpandedDescendants {
            stack,
            namespace_uri,
            local_name,
        }
    }

    pub(crate) fn text(&self) -> String {
        let mut output = String::new();
        self.append_text(&mut output);
        output
    }

    fn append_text(&self, output: &mut String) {
        for content in &self.content {
            match content {
                XmlContent::Text(text) => output.push_str(text),
                XmlContent::Element(child) => child.append_text(output),
            }
        }
    }
}

pub(crate) struct ExpandedDescendants<'a> {
    stack: Vec<&'a XmlNode>,
    namespace_uri: &'a str,
    local_name: &'a str,
}

impl<'a> Iterator for ExpandedDescendants<'a> {
    type Item = &'a XmlNode;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(node) = self.stack.pop() {
            let mut children: Vec<_> = node.children().collect();
            children.reverse();
            self.stack.extend(children);
            if node.is(self.namespace_uri, self.local_name) {
                return Some(node);
            }
        }
        None
    }
}

#[derive(Debug)]
struct XmlFrame {
    node: XmlNode,
    namespace_bindings: Vec<NamespaceBinding>,
}

#[derive(Debug)]
struct NamespaceBinding {
    prefix: String,
    uri: String,
}

#[derive(Debug)]
pub(crate) enum XmlContent {
    Text(String),
    Element(XmlNode),
}

pub(crate) struct ParsedXml {
    pub(crate) roots: Vec<XmlNode>,
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

    fn check_raw_text(&self, bytes: usize) -> Result<(), ConversionError> {
        let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
        let Some(actual) = self.text_bytes.checked_add(bytes) else {
            return Err(limit_exceeded(
                "xml_text_bytes",
                u64::MAX,
                self.limits.max_xml_text_bytes,
            ));
        };
        if actual > self.limits.max_xml_text_bytes {
            return Err(limit_exceeded(
                "xml_text_bytes",
                actual,
                self.limits.max_xml_text_bytes,
            ));
        }
        Ok(())
    }
}

pub(crate) fn parse_xml(
    input: &str,
    limits: &StructuredLimits,
) -> Result<ParsedXml, ConversionError> {
    validate_xml_chars(input, "document")?;
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
                let frame = frame_from_start(&reader, &start, &mut budget, &stack)?;
                stack.push(frame);
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
                let frame = frame_from_start(&reader, &start, &mut budget, &stack)?;
                append_node(frame.node, &mut stack, &mut roots);
                if stack.is_empty() {
                    phase = DocumentPhase::Epilog;
                }
            }
            Event::End(_) => {
                if phase != DocumentPhase::Root {
                    return corrupt("XML closing element is outside the root element");
                }
                let frame = stack.pop().ok_or_else(|| ConversionError::CorruptInput {
                    message: "XML closing element has no matching start element".into(),
                })?;
                append_node(frame.node, &mut stack, &mut roots);
                if stack.is_empty() {
                    phase = DocumentPhase::Epilog;
                }
            }
            Event::Text(text) => {
                budget.check_raw_text(text.as_ref().len())?;
                let value = text
                    .xml_content(XmlVersion::Implicit1_0)
                    .map_err(|error| ConversionError::CorruptInput {
                        message: format!("invalid XML text encoding: {error}"),
                    })?
                    .into_owned();
                validate_xml_chars(&value, "text")?;
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
                budget.check_raw_text(text.as_ref().len())?;
                let value = text
                    .xml_content(XmlVersion::Implicit1_0)
                    .map_err(|error| ConversionError::CorruptInput {
                        message: format!("invalid XML CDATA encoding: {error}"),
                    })?
                    .into_owned();
                validate_xml_chars(&value, "CDATA")?;
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
                let value = decode_general_reference(&name)?;
                budget.add_text(value.len_utf8())?;
                append_text(value.to_string(), &mut stack);
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
    let attributes = declaration_attributes(declaration.as_ref())?;
    if attributes.first().map(|(name, _)| *name) != Some(b"version".as_slice()) {
        return corrupt("XML declaration version must be the first pseudo-attribute");
    }
    if attributes[0].1 != b"1.0" {
        return corrupt("only XML version 1.0 is supported");
    }
    let mut index = 1;
    if attributes.get(index).map(|(name, _)| *name) == Some(b"encoding".as_slice()) {
        validate_encoding(attributes[index].1)?;
        index += 1;
    }
    if attributes.get(index).map(|(name, _)| *name) == Some(b"standalone".as_slice()) {
        if !matches!(attributes[index].1, b"yes" | b"no") {
            return corrupt("XML standalone declaration must be yes or no");
        }
        index += 1;
    }
    if index != attributes.len() {
        return corrupt(
            "XML declaration allows only version, optional encoding, then optional standalone",
        );
    }
    Ok(())
}

type DeclarationAttribute<'a> = (&'a [u8], &'a [u8]);

fn declaration_attributes(raw: &[u8]) -> Result<Vec<DeclarationAttribute<'_>>, ConversionError> {
    if !raw.starts_with(b"xml") {
        return corrupt("invalid XML declaration target");
    }
    let mut position = 3usize;
    let mut attributes = Vec::new();
    while position < raw.len() {
        let whitespace_start = position;
        while raw.get(position).is_some_and(|byte| is_xml_space(*byte)) {
            position += 1;
        }
        if position == raw.len() {
            break;
        }
        if position == whitespace_start {
            return corrupt("XML declaration pseudo-attributes must be separated by whitespace");
        }

        let name_start = position;
        while raw.get(position).is_some_and(u8::is_ascii_alphabetic) {
            position += 1;
        }
        if position == name_start {
            return corrupt("invalid XML declaration pseudo-attribute name");
        }
        let name = &raw[name_start..position];
        while raw.get(position).is_some_and(|byte| is_xml_space(*byte)) {
            position += 1;
        }
        if raw.get(position) != Some(&b'=') {
            return corrupt("XML declaration pseudo-attribute is missing '='");
        }
        position += 1;
        while raw.get(position).is_some_and(|byte| is_xml_space(*byte)) {
            position += 1;
        }
        let quote = *raw
            .get(position)
            .ok_or_else(|| ConversionError::CorruptInput {
                message: "XML declaration pseudo-attribute is missing a value".into(),
            })?;
        if quote != b'\'' && quote != b'"' {
            return corrupt("XML declaration values must be quoted");
        }
        position += 1;
        let value_start = position;
        while raw.get(position).is_some_and(|byte| *byte != quote) {
            position += 1;
        }
        let value =
            raw.get(value_start..position)
                .ok_or_else(|| ConversionError::CorruptInput {
                    message: "invalid XML declaration value".into(),
                })?;
        if raw.get(position) != Some(&quote) {
            return corrupt("unterminated XML declaration value");
        }
        position += 1;
        if attributes.len() == 3 {
            return corrupt("XML declaration has too many pseudo-attributes");
        }
        attributes.push((name, value));
    }
    if attributes.is_empty() {
        return corrupt("XML declaration is missing version");
    }
    Ok(attributes)
}

fn validate_encoding(encoding: &[u8]) -> Result<(), ConversionError> {
    let legal_name = encoding.first().is_some_and(u8::is_ascii_alphabetic)
        && encoding
            .iter()
            .skip(1)
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
    if !legal_name {
        return corrupt("XML declaration contains an invalid encoding name");
    }
    if !encoding.eq_ignore_ascii_case(b"UTF-8") {
        return corrupt("XML declaration encoding must be UTF-8");
    }
    Ok(())
}

fn is_xml_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

fn validate_processing_instruction(instruction: &BytesPI<'_>) -> Result<(), ConversionError> {
    let target = std::str::from_utf8(instruction.target()).map_err(|error| {
        ConversionError::CorruptInput {
            message: format!("processing instruction target is not UTF-8: {error}"),
        }
    })?;
    if !valid_ncname(target) {
        return corrupt(format!(
            "processing instruction target {target:?} is not an XML NCName"
        ));
    }
    if target.eq_ignore_ascii_case("xml") {
        return corrupt("processing instruction target XML is reserved");
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QNameKind {
    Element,
    Attribute,
}

fn validate_qname(name: &str, kind: QNameKind) -> Result<(), ConversionError> {
    let mut parts = name.split(':');
    let first = parts.next().unwrap_or_default();
    let second = parts.next();
    if parts.next().is_some() || first.is_empty() || second.is_some_and(str::is_empty) {
        return corrupt(format!("invalid XML QName {name:?}"));
    }
    if !valid_ncname(first) || second.is_some_and(|local| !valid_ncname(local)) {
        return corrupt(format!("invalid XML QName {name:?}"));
    }

    if let Some(prefix) = second.map(|_| first)
        && prefix == "xmlns"
        && kind != QNameKind::Attribute
    {
        return corrupt(format!("reserved XMLNS prefix is invalid in {name:?}"));
    }
    Ok(())
}

fn valid_ncname(name: &str) -> bool {
    let mut characters = name.chars();
    characters.next().is_some_and(is_name_start_character) && characters.all(is_name_character)
}

fn is_name_start_character(character: char) -> bool {
    matches!(character,
        'A'..='Z' | '_' | 'a'..='z'
        | '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}' | '\u{F8}'..='\u{2FF}'
        | '\u{370}'..='\u{37D}' | '\u{37F}'..='\u{1FFF}'
        | '\u{200C}'..='\u{200D}' | '\u{2070}'..='\u{218F}'
        | '\u{2C00}'..='\u{2FEF}' | '\u{3001}'..='\u{D7FF}'
        | '\u{F900}'..='\u{FDCF}' | '\u{FDF0}'..='\u{FFFD}'
        | '\u{10000}'..='\u{EFFFF}'
    )
}

fn is_name_character(character: char) -> bool {
    is_name_start_character(character)
        || matches!(character,
            '-' | '.' | '0'..='9' | '\u{B7}' | '\u{300}'..='\u{36F}'
            | '\u{203F}'..='\u{2040}'
        )
}

fn validate_namespace_binding(prefix: &str, value: &str) -> Result<(), ConversionError> {
    const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";
    const XMLNS_NAMESPACE: &str = "http://www.w3.org/2000/xmlns/";
    if prefix == "xmlns" || value == XMLNS_NAMESPACE {
        return corrupt("the XMLNS namespace cannot be rebound");
    }
    if !prefix.is_empty() && value.is_empty() {
        return corrupt("a prefixed namespace binding cannot have an empty URI");
    }
    if (prefix == "xml") != (value == XML_NAMESPACE) {
        return corrupt("the XML prefix must bind only to its reserved namespace");
    }
    if prefix.is_empty() && (value == XML_NAMESPACE || value == XMLNS_NAMESPACE) {
        return corrupt("reserved XML namespaces cannot be the default namespace");
    }
    Ok(())
}

fn validate_xml_chars(value: &str, context: &str) -> Result<(), ConversionError> {
    if let Some(character) = value
        .chars()
        .find(|character| !is_xml_1_0_character(*character))
    {
        return corrupt(format!(
            "illegal XML 1.0 character U+{:04X} in {context}",
            u32::from(character)
        ));
    }
    Ok(())
}

fn is_xml_1_0_character(character: char) -> bool {
    matches!(character,
        '\u{9}' | '\u{A}' | '\u{D}'
        | '\u{20}'..='\u{D7FF}'
        | '\u{E000}'..='\u{FFFD}'
        | '\u{10000}'..='\u{10FFFF}'
    )
}

fn decode_general_reference(name: &str) -> Result<char, ConversionError> {
    let character = if let Some(hex) = name.strip_prefix("#x") {
        u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
    } else if let Some(decimal) = name.strip_prefix('#') {
        decimal.parse::<u32>().ok().and_then(char::from_u32)
    } else {
        match name {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "apos" => Some('\''),
            "quot" => Some('"'),
            _ => None,
        }
    }
    .ok_or_else(|| ConversionError::CorruptInput {
        message: format!("unsafe or unknown XML entity &{name};"),
    })?;
    if !is_xml_1_0_character(character) {
        return corrupt(format!(
            "illegal XML 1.0 character reference &{name}; (U+{:04X})",
            u32::from(character)
        ));
    }
    Ok(character)
}

fn frame_from_start(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    budget: &mut XmlBudget<'_>,
    ancestors: &[XmlFrame],
) -> Result<XmlFrame, ConversionError> {
    let name = std::str::from_utf8(start.name().as_ref())
        .map_err(|error| ConversionError::CorruptInput {
            message: format!("XML element name is not UTF-8: {error}"),
        })?
        .to_owned();
    validate_qname(&name, QNameKind::Element)?;
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
        budget.check_raw_text(attribute.value.as_ref().len())?;
        let key = std::str::from_utf8(attribute.key.as_ref())
            .map_err(|error| ConversionError::CorruptInput {
                message: format!("XML attribute name is not UTF-8: {error}"),
            })?
            .to_owned();
        validate_qname(&key, QNameKind::Attribute)?;
        let value = attribute
            .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
            .map_err(|error| ConversionError::CorruptInput {
                message: format!("invalid or unsafe XML attribute value: {error}"),
            })?
            .into_owned();
        validate_xml_chars(&value, "attribute value")?;
        budget.add_text(value.len())?;
        attributes.push((key, value));
    }

    let namespace_bindings = namespace_bindings(&attributes)?;
    validate_element_namespace(&name, &namespace_bindings, ancestors)?;
    validate_expanded_attribute_names(&attributes, &namespace_bindings, ancestors)?;
    let (namespace_uri, local_name) = expanded_element_name(&name, &namespace_bindings, ancestors)?;
    let expanded_attributes = expanded_attributes(&attributes, &namespace_bindings, ancestors)?;
    Ok(XmlFrame {
        node: XmlNode {
            name,
            attributes,
            namespace_uri,
            local_name,
            expanded_attributes,
            content: Vec::new(),
        },
        namespace_bindings,
    })
}

fn expanded_element_name(
    name: &str,
    current: &[NamespaceBinding],
    ancestors: &[XmlFrame],
) -> Result<(Option<String>, String), ConversionError> {
    let (prefix, local) = name
        .split_once(':')
        .map_or(("", name), |(prefix, local)| (prefix, local));
    let namespace = resolve_namespace(prefix, current, ancestors)
        .filter(|uri| !uri.is_empty())
        .map(ToOwned::to_owned);
    if !prefix.is_empty() && namespace.is_none() {
        return corrupt(format!(
            "element prefix {prefix:?} is not declared in scope"
        ));
    }
    Ok((namespace, local.to_owned()))
}

fn expanded_attributes(
    attributes: &[(String, String)],
    current: &[NamespaceBinding],
    ancestors: &[XmlFrame],
) -> Result<Vec<ExpandedAttribute>, ConversionError> {
    attributes
        .iter()
        .filter(|(name, _)| name != "xmlns" && !name.starts_with("xmlns:"))
        .map(|(name, value)| {
            let (namespace_uri, local_name) = if let Some((prefix, local)) = name.split_once(':') {
                let uri = resolve_namespace(prefix, current, ancestors).ok_or_else(|| {
                    ConversionError::CorruptInput {
                        message: format!("attribute prefix {prefix:?} is not declared in scope"),
                    }
                })?;
                (Some(uri.to_owned()), local.to_owned())
            } else {
                (None, name.clone())
            };
            Ok(ExpandedAttribute {
                namespace_uri,
                local_name,
                value: value.clone(),
            })
        })
        .collect()
}

fn namespace_bindings(
    attributes: &[(String, String)],
) -> Result<Vec<NamespaceBinding>, ConversionError> {
    let mut bindings = Vec::new();
    let mut declared = HashSet::new();
    for (name, value) in attributes {
        let prefix = if name == "xmlns" {
            Some("")
        } else {
            name.strip_prefix("xmlns:")
        };
        let Some(prefix) = prefix else {
            continue;
        };
        if !declared.insert(prefix) {
            return corrupt(format!(
                "namespace prefix {prefix:?} is declared more than once on an element"
            ));
        }
        validate_namespace_binding(prefix, value)?;
        bindings.push(NamespaceBinding {
            prefix: prefix.to_owned(),
            uri: value.clone(),
        });
    }
    Ok(bindings)
}

fn validate_element_namespace(
    name: &str,
    current: &[NamespaceBinding],
    ancestors: &[XmlFrame],
) -> Result<(), ConversionError> {
    if let Some((prefix, _)) = name.split_once(':')
        && resolve_namespace(prefix, current, ancestors).is_none()
    {
        return corrupt(format!(
            "element prefix {prefix:?} is not declared in scope"
        ));
    }
    Ok(())
}

fn validate_expanded_attribute_names(
    attributes: &[(String, String)],
    current: &[NamespaceBinding],
    ancestors: &[XmlFrame],
) -> Result<(), ConversionError> {
    let mut expanded = HashSet::new();
    for (name, _) in attributes {
        if name == "xmlns" || name.starts_with("xmlns:") {
            continue;
        }
        let (namespace, local) = if let Some((prefix, local)) = name.split_once(':') {
            let namespace = resolve_namespace(prefix, current, ancestors).ok_or_else(|| {
                ConversionError::CorruptInput {
                    message: format!("attribute prefix {prefix:?} is not declared in scope"),
                }
            })?;
            (Some(namespace.to_owned()), local)
        } else {
            (None, name.as_str())
        };
        if !expanded.insert((namespace, local.to_owned())) {
            return corrupt(format!(
                "duplicate expanded XML attribute name for local name {local:?}"
            ));
        }
    }
    Ok(())
}

fn resolve_namespace<'a>(
    prefix: &str,
    current: &'a [NamespaceBinding],
    ancestors: &'a [XmlFrame],
) -> Option<&'a str> {
    const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";
    if prefix == "xml" {
        return Some(XML_NAMESPACE);
    }
    current
        .iter()
        .rev()
        .find(|binding| binding.prefix == prefix)
        .map(|binding| binding.uri.as_str())
        .or_else(|| {
            ancestors.iter().rev().find_map(|frame| {
                frame
                    .namespace_bindings
                    .iter()
                    .rev()
                    .find(|binding| binding.prefix == prefix)
                    .map(|binding| binding.uri.as_str())
            })
        })
}

fn append_node(node: XmlNode, stack: &mut [XmlFrame], roots: &mut Vec<XmlNode>) {
    if let Some(parent) = stack.last_mut() {
        parent.node.content.push(XmlContent::Element(node));
    } else {
        roots.push(node);
    }
}

fn append_text(text: String, stack: &mut [XmlFrame]) {
    let parent = stack
        .last_mut()
        .expect("document grammar ensures text has an open element");
    if let Some(XmlContent::Text(existing)) = parent.node.content.last_mut() {
        existing.push_str(&text);
    } else {
        parent.node.content.push(XmlContent::Text(text));
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
