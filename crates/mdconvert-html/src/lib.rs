mod convert;
mod dom;

use std::collections::HashMap;

use mdconvert_core::{AssetId, ConversionError, ConversionRequest, Converter, Document};

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

    /// Converts bounded HTML bytes whose selected image sources already refer
    /// to caller-owned local assets. No source is decoded or loaded again.
    pub fn convert_bytes_with_asset_refs(
        &self,
        bytes: &[u8],
        request: &ConversionRequest,
        asset_refs: &HashMap<String, AssetId>,
    ) -> Result<Document, ConversionError> {
        let dom = dom::parse_bytes(bytes, request.limits.max_input_bytes)?;
        convert::document_from_dom_with_asset_refs(dom, request, asset_refs)
    }
}
