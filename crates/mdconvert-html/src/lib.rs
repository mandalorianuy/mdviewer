mod convert;
mod dom;

use mdconvert_core::{ConversionError, ConversionRequest, Converter, Document};

#[derive(Debug, Default, Clone, Copy)]
pub struct HtmlConverter;

impl Converter for HtmlConverter {
    fn convert(&self, request: &ConversionRequest) -> Result<Document, ConversionError> {
        let dom = dom::parse_file(&request.source, request.limits.max_input_bytes)?;
        convert::document_from_dom(dom, request)
    }
}
