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

impl HtmlConverter {
    /// Converts already-bounded HTML bytes while retaining the request as the
    /// local provenance root. Container formats use this without extracting to disk.
    pub fn convert_bytes(
        &self,
        bytes: &[u8],
        request: &ConversionRequest,
    ) -> Result<Document, ConversionError> {
        let dom = dom::parse_bytes(bytes, request.limits.max_input_bytes)?;
        convert::document_from_dom(dom, request)
    }
}
