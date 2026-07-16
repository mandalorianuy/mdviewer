use crate::{ConversionError, ConversionRequest, Document};

pub trait Converter: Send + Sync {
    fn convert(&self, request: &ConversionRequest) -> Result<Document, ConversionError>;
}
