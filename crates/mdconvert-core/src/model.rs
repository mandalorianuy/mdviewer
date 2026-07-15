use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::ModelError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub metadata: DocumentMetadata,
    pub blocks: Vec<Block>,
    pub assets: Vec<Asset>,
    pub warnings: Vec<ConversionWarning>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DocumentMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub source_format: Option<String>,
    pub page_count: Option<u32>,
    pub properties: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AssetId(String);

impl AssetId {
    pub fn new(value: impl Into<String>) -> Result<Self, ModelError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ModelError::EmptyAssetId);
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    pub id: AssetId,
    pub file_name: String,
    pub media_type: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListItem {
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Alignment {
    None,
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Block {
    Heading {
        level: u8,
        content: Vec<Inline>,
    },
    Paragraph {
        content: Vec<Inline>,
    },
    List {
        ordered: bool,
        start: Option<u64>,
        items: Vec<ListItem>,
    },
    Table {
        alignments: Vec<Alignment>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
    Code {
        language: Option<String>,
        text: String,
    },
    Quote {
        blocks: Vec<Block>,
    },
    Image {
        asset_id: AssetId,
        alt: String,
    },
    ThematicBreak,
}

impl Block {
    pub fn heading(level: u8, content: Vec<Inline>) -> Result<Self, ModelError> {
        if !(1..=6).contains(&level) {
            return Err(ModelError::InvalidHeadingLevel(level));
        }

        Ok(Self::Heading { level, content })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Inline {
    Text(String),
    Emphasis(Vec<Inline>),
    Strong(Vec<Inline>),
    Code(String),
    Link {
        url: String,
        title: Option<String>,
        content: Vec<Inline>,
    },
    LineBreak,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningCode {
    AmbiguousReadingOrder,
    TableDegraded,
    FontMetadataInsufficient,
    MissingImageAlt,
    InvalidLinkSkipped,
    InvalidAssetSkipped,
    ExternalAssetSkipped,
    OcrDeferred,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversionWarning {
    pub code: WarningCode,
    pub message: String,
    /// The 1-based page number associated with the warning, when available.
    pub page: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ConversionLimits {
    pub max_input_bytes: u64,
    pub max_pages: u32,
    pub max_assets: u32,
}

impl Default for ConversionLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 500 * 1024 * 1024,
            max_pages: 2_000,
            max_assets: 10_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversionRequest {
    pub source: PathBuf,
    pub source_url: Option<url::Url>,
    pub limits: ConversionLimits,
}

impl ConversionRequest {
    pub fn new(source: impl Into<PathBuf>) -> Result<Self, ModelError> {
        let source = source.into();
        if source.as_os_str().is_empty() {
            return Err(ModelError::EmptySourcePath);
        }

        Ok(Self {
            source,
            source_url: None,
            limits: ConversionLimits::default(),
        })
    }
}
