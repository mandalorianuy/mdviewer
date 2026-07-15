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
    de::{DeserializeSeed, MapAccess, SeqAccess, Visitor},
};

use crate::{
    StructuredFormat, StructuredLimits, ensure_format, limit_exceeded, read_input, strip_utf8_bom,
    utf8,
};

const SERDE_JSON_NUMBER_TOKEN: &str = "$serde_json::private::Number";

#[derive(Debug, Default, Clone, Copy)]
pub struct JsonConverter;

impl JsonConverter {
    pub fn convert_with_limits(
        &self,
        request: &ConversionRequest,
        limits: &StructuredLimits,
    ) -> Result<Document, ConversionError> {
        limits.validate()?;
        let bytes = read_input(request)?;
        ensure_format(request, &bytes, StructuredFormat::Json, limits)?;
        let input = utf8(strip_utf8_bom(&bytes), &request.source)?;
        if contains_reserved_number_key(input) {
            return Err(ConversionError::CorruptInput {
                message: format!(
                    "JSON object key {SERDE_JSON_NUMBER_TOKEN:?} is reserved by the exact-number parser"
                ),
            });
        }
        enforce_nesting_limit(input, limits.max_json_depth)?;
        let mut deserializer = serde_json::Deserializer::from_str(input);
        deserializer.disable_recursion_limit();
        let mut budget = JsonBudget::new(limits);
        let parsed = JsonSeed {
            budget: &mut budget,
        }
        .deserialize(&mut deserializer);
        let value = match parsed {
            Ok(value) => value,
            Err(error) => {
                if let Some(exceeded) = budget.exceeded {
                    return Err(exceeded.into_conversion());
                }
                return Err(json_error(error));
            }
        };
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

impl Converter for JsonConverter {
    fn convert(&self, request: &ConversionRequest) -> Result<Document, ConversionError> {
        self.convert_with_limits(request, &StructuredLimits::default())
    }
}

pub(crate) fn validate_json_candidate(
    input: &str,
    limits: &StructuredLimits,
) -> Result<bool, ConversionError> {
    enforce_nesting_limit(input, limits.max_json_depth)?;
    let mut deserializer = serde_json::Deserializer::from_str(input);
    deserializer.disable_recursion_limit();
    let parsed = serde::de::IgnoredAny::deserialize(&mut deserializer);
    Ok(parsed.is_ok() && deserializer.end().is_ok())
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

#[derive(Debug, Clone, Copy)]
struct BudgetExceeded {
    limit: &'static str,
    actual: u64,
    maximum: u64,
}

impl BudgetExceeded {
    fn into_conversion(self) -> ConversionError {
        limit_exceeded(self.limit, self.actual, self.maximum)
    }
}

struct JsonBudget<'a> {
    limits: &'a StructuredLimits,
    nodes: u64,
    exceeded: Option<BudgetExceeded>,
}

impl<'a> JsonBudget<'a> {
    fn new(limits: &'a StructuredLimits) -> Self {
        Self {
            limits,
            nodes: 0,
            exceeded: None,
        }
    }

    fn add_node<E: serde::de::Error>(&mut self) -> Result<(), E> {
        let actual = match self.nodes.checked_add(1) {
            Some(actual) => actual,
            None => {
                self.exceeded = Some(BudgetExceeded {
                    limit: "json_nodes",
                    actual: u64::MAX,
                    maximum: self.limits.max_json_nodes,
                });
                return Err(E::custom("JSON node counter overflowed"));
            }
        };
        if actual > self.limits.max_json_nodes {
            let exceeded = BudgetExceeded {
                limit: "json_nodes",
                actual,
                maximum: self.limits.max_json_nodes,
            };
            self.exceeded = Some(exceeded);
            return Err(E::custom("JSON node budget exceeded"));
        }
        self.nodes = actual;
        Ok(())
    }

    fn check_entries<E: serde::de::Error>(
        &mut self,
        limit: &'static str,
        actual: u64,
        maximum: u64,
    ) -> Result<(), E> {
        if actual > maximum {
            let exceeded = BudgetExceeded {
                limit,
                actual,
                maximum,
            };
            self.exceeded = Some(exceeded);
            return Err(E::custom(format!("{limit} budget exceeded")));
        }
        Ok(())
    }
}

struct JsonSeed<'a, 'limits> {
    budget: &'a mut JsonBudget<'limits>,
}

impl<'de> DeserializeSeed<'de> for JsonSeed<'_, '_> {
    type Value = JsonValue;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        self.budget.add_node()?;
        deserializer.deserialize_any(JsonVisitor {
            budget: self.budget,
        })
    }
}

struct JsonVisitor<'a, 'limits> {
    budget: &'a mut JsonBudget<'limits>,
}

impl<'de> Visitor<'de> for JsonVisitor<'_, '_> {
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
        let mut values = Vec::new();
        let mut entries = 0u64;
        while let Some(value) = sequence.next_element_seed(JsonSeed {
            budget: self.budget,
        })? {
            entries = match entries.checked_add(1) {
                Some(actual) => actual,
                None => {
                    self.budget.exceeded = Some(BudgetExceeded {
                        limit: "json_array_entries",
                        actual: u64::MAX,
                        maximum: self.budget.limits.max_json_array_entries,
                    });
                    return Err(serde::de::Error::custom(
                        "json_array_entries counter overflowed",
                    ));
                }
            };
            self.budget.check_entries(
                "json_array_entries",
                entries,
                self.budget.limits.max_json_array_entries,
            )?;
            values.push(value);
        }
        Ok(JsonValue::Array(values))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = Vec::new();
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
        let mut entries = 1u64;
        self.budget.check_entries(
            "json_object_entries",
            entries,
            self.budget.limits.max_json_object_entries,
        )?;
        keys.insert(first_key.clone());
        values.push((
            first_key,
            map.next_value_seed(JsonSeed {
                budget: self.budget,
            })?,
        ));
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key.clone()) {
                return Err(serde::de::Error::custom(format!(
                    "duplicate JSON object key {key:?}"
                )));
            }
            entries = match entries.checked_add(1) {
                Some(actual) => actual,
                None => {
                    self.budget.exceeded = Some(BudgetExceeded {
                        limit: "json_object_entries",
                        actual: u64::MAX,
                        maximum: self.budget.limits.max_json_object_entries,
                    });
                    return Err(serde::de::Error::custom(
                        "json_object_entries counter overflowed",
                    ));
                }
            };
            self.budget.check_entries(
                "json_object_entries",
                entries,
                self.budget.limits.max_json_object_entries,
            )?;
            values.push((
                key,
                map.next_value_seed(JsonSeed {
                    budget: self.budget,
                })?,
            ));
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

fn enforce_nesting_limit(input: &str, maximum: u64) -> Result<(), ConversionError> {
    let mut depth = 0u64;
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
                depth = match depth.checked_add(1) {
                    Some(actual) => actual,
                    None => {
                        return Err(limit_exceeded("json_nesting_depth", u64::MAX, maximum));
                    }
                };
                if depth > maximum {
                    return Err(ConversionError::LimitExceeded {
                        limit: "json_nesting_depth",
                        actual: depth,
                        maximum,
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
