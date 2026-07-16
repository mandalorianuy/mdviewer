use std::path::PathBuf;

use mdconvert_core::{ConversionWarning, DocumentMetadata};
use serde::Serialize;

pub const SCHEMA_VERSION: &str = "mdviewer.convert/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErrorObject {
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResultEnvelope {
    pub schema_version: &'static str,
    pub status: Status,
    pub markdown_path: Option<PathBuf>,
    pub assets_path: Option<PathBuf>,
    pub metadata: ResultMetadata,
    pub warnings: Vec<ConversionWarning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ResultMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_count: Option<u32>,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub properties: std::collections::BTreeMap<String, String>,
}

impl From<DocumentMetadata> for ResultMetadata {
    fn from(value: DocumentMetadata) -> Self {
        Self {
            title: value.title,
            author: value.author,
            subject: value.subject,
            source_format: value.source_format,
            page_count: value.page_count,
            properties: value.properties,
        }
    }
}

impl ResultEnvelope {
    pub fn succeeded(
        markdown_path: PathBuf,
        assets_path: Option<PathBuf>,
        metadata: DocumentMetadata,
        warnings: Vec<ConversionWarning>,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            status: Status::Succeeded,
            markdown_path: Some(markdown_path),
            assets_path,
            metadata: metadata.into(),
            warnings,
            error: None,
        }
    }

    pub fn failed(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            status: Status::Failed,
            markdown_path: None,
            assets_path: None,
            metadata: ResultMetadata::default(),
            warnings: Vec::new(),
            error: Some(ErrorObject {
                code,
                message: message.into(),
            }),
        }
    }
}
