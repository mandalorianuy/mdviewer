use std::{
    collections::{BTreeMap, HashSet},
    fmt,
};

use mdconvert_core::{
    Block, ConversionError, ConversionRequest, Converter, Document, DocumentMetadata, Inline,
    ListItem,
};
use serde::{
    Deserialize, Deserializer,
    de::{MapAccess, SeqAccess, Visitor},
};

use crate::{StructuredFormat, ensure_format, read_input, strip_utf8_bom, utf8};

const MAX_JSON_DEPTH: usize = 128;
const SERDE_JSON_NUMBER_TOKEN: &str = "$serde_json::private::Number";

#[derive(Debug, Default, Clone, Copy)]
pub struct JsonConverter;

impl Converter for JsonConverter {
    fn convert(&self, request: &ConversionRequest) -> Result<Document, ConversionError> {
        let bytes = read_input(request)?;
        ensure_format(request, &bytes, StructuredFormat::Json)?;
        let input = utf8(strip_utf8_bom(&bytes), &request.source)?;
        if contains_reserved_number_key(input) {
            return Err(ConversionError::CorruptInput {
                message: format!(
                    "JSON object key {SERDE_JSON_NUMBER_TOKEN:?} is reserved by the exact-number parser"
                ),
            });
        }
        enforce_nesting_limit(input, MAX_JSON_DEPTH)?;
        let mut deserializer = serde_json::Deserializer::from_str(input);
        let value = JsonValue::deserialize(&mut deserializer).map_err(json_error)?;
        deserializer.end().map_err(json_error)?;

        let mut properties = BTreeMap::new();
        properties.insert("object_key_order".into(), "source".into());
        Ok(Document {
            metadata: DocumentMetadata {
                source_format: Some("json".into()),
                properties,
                ..DocumentMetadata::default()
            },
            blocks: value_blocks(&value),
            assets: Vec::new(),
            warnings: Vec::new(),
        })
    }
}

fn contains_reserved_number_key(input: &str) -> bool {
    let bytes = input.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'"' {
            index += 1;
            continue;
        }
        let start = index;
        index += 1;
        let mut escaped = false;
        while index < bytes.len() {
            let byte = bytes[index];
            index += 1;
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                break;
            }
        }
        let decoded = serde_json::from_str::<String>(&input[start..index]);
        if !decoded.is_ok_and(|value| value == SERDE_JSON_NUMBER_TOKEN) {
            continue;
        }
        let mut lookahead = index;
        while lookahead < bytes.len() && bytes[lookahead].is_ascii_whitespace() {
            lookahead += 1;
        }
        if bytes.get(lookahead) == Some(&b':') {
            return true;
        }
    }
    false
}

#[derive(Debug)]
enum JsonValue {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    Array(Vec<Self>),
    Object(Vec<(String, Self)>),
}

impl<'de> Deserialize<'de> for JsonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(JsonVisitor)
    }
}

struct JsonVisitor;

impl<'de> Visitor<'de> for JsonVisitor {
    type Value = JsonValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(JsonValue::Null)
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(JsonValue::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(JsonValue::Number(value.into()))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(JsonValue::Number(value.into()))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(JsonValue::Number)
            .ok_or_else(|| E::custom("JSON number is not finite"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(JsonValue::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(JsonValue::String(value))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::with_capacity(sequence.size_hint().unwrap_or(0));
        while let Some(value) = sequence.next_element()? {
            values.push(value);
        }
        Ok(JsonValue::Array(values))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = Vec::with_capacity(map.size_hint().unwrap_or(0));
        let mut keys = HashSet::new();
        let Some(first_key) = map.next_key::<String>()? else {
            return Ok(JsonValue::Object(values));
        };
        if first_key == SERDE_JSON_NUMBER_TOKEN {
            let raw = map.next_value::<String>()?;
            if map.next_key::<serde::de::IgnoredAny>()?.is_some() {
                return Err(serde::de::Error::custom(
                    "invalid arbitrary-precision JSON number representation",
                ));
            }
            let number = serde_json::from_str::<serde_json::Number>(&raw)
                .map_err(serde::de::Error::custom)?;
            return Ok(JsonValue::Number(number));
        }
        keys.insert(first_key.clone());
        values.push((first_key, map.next_value()?));
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key.clone()) {
                return Err(serde::de::Error::custom(format!(
                    "duplicate JSON object key {key:?}"
                )));
            }
            values.push((key, map.next_value()?));
        }
        Ok(JsonValue::Object(values))
    }
}

fn json_error(error: serde_json::Error) -> ConversionError {
    ConversionError::CorruptInput {
        message: format!(
            "invalid JSON at line {}, column {}: {error}",
            error.line(),
            error.column()
        ),
    }
}

fn enforce_nesting_limit(input: &str, maximum: usize) -> Result<(), ConversionError> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for character in input.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' | '[' => {
                depth = depth.saturating_add(1);
                if depth > maximum {
                    return Err(ConversionError::LimitExceeded {
                        limit: "json_nesting_depth",
                        actual: u64::try_from(depth).unwrap_or(u64::MAX),
                        maximum: u64::try_from(maximum).unwrap_or(u64::MAX),
                    });
                }
            }
            '}' | ']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

fn value_blocks(value: &JsonValue) -> Vec<Block> {
    match value {
        JsonValue::Object(entries) if !entries.is_empty() => vec![object_list(entries)],
        JsonValue::Array(values) if !values.is_empty() => vec![array_list(values)],
        _ => vec![Block::Paragraph {
            content: scalar_inlines(value),
        }],
    }
}

fn object_list(entries: &[(String, JsonValue)]) -> Block {
    Block::List {
        ordered: false,
        start: None,
        items: entries
            .iter()
            .map(|(key, value)| ListItem {
                blocks: object_field_blocks(key, value),
            })
            .collect(),
    }
}

fn object_field_blocks(key: &str, value: &JsonValue) -> Vec<Block> {
    let mut content = vec![Inline::Strong(vec![Inline::Text(key.to_owned())])];
    content.push(Inline::Text(": ".into()));
    let mut blocks = Vec::new();
    match value {
        JsonValue::Object(entries) if !entries.is_empty() => {
            content.pop();
            content.push(Inline::Text(":".into()));
            blocks.push(Block::Paragraph { content });
            blocks.push(object_list(entries));
        }
        JsonValue::Array(values) if !values.is_empty() => {
            content.pop();
            content.push(Inline::Text(":".into()));
            blocks.push(Block::Paragraph { content });
            blocks.push(array_list(values));
        }
        _ => {
            content.extend(scalar_inlines(value));
            blocks.push(Block::Paragraph { content });
        }
    }
    blocks
}

fn array_list(values: &[JsonValue]) -> Block {
    Block::List {
        ordered: false,
        start: None,
        items: values
            .iter()
            .map(|value| match value {
                JsonValue::Object(entries) if !entries.is_empty() => ListItem {
                    blocks: entries
                        .iter()
                        .flat_map(|(key, value)| object_field_blocks(key, value))
                        .collect(),
                },
                JsonValue::Array(values) if !values.is_empty() => ListItem {
                    blocks: vec![array_list(values)],
                },
                _ => ListItem {
                    blocks: vec![Block::Paragraph {
                        content: scalar_inlines(value),
                    }],
                },
            })
            .collect(),
    }
}

fn scalar_inlines(value: &JsonValue) -> Vec<Inline> {
    match value {
        JsonValue::Null => vec![Inline::Code("null".into())],
        JsonValue::Bool(value) => vec![Inline::Code(value.to_string())],
        JsonValue::Number(value) => vec![Inline::Code(value.to_string())],
        JsonValue::String(value) => text_inlines(value),
        JsonValue::Array(values) if values.is_empty() => vec![Inline::Code("[]".into())],
        JsonValue::Object(entries) if entries.is_empty() => vec![Inline::Code("{}".into())],
        JsonValue::Array(_) | JsonValue::Object(_) => {
            unreachable!("non-empty containers are blocks")
        }
    }
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
