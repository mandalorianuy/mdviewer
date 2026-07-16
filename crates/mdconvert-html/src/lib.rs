mod convert;
mod dom;

use std::cell::Cell;
use std::collections::HashMap;

use html5ever::{
    tendril::StrTendril,
    tokenizer::{BufferQueue, Token, TokenSink, TokenSinkResult, Tokenizer},
};
use mdconvert_core::{AssetId, ConversionError, ConversionRequest, Converter, Document};

#[derive(Debug, Default, Clone, Copy)]
pub struct HtmlConverter;

struct HtmlSignalSink(Cell<bool>);

impl TokenSink for HtmlSignalSink {
    type Handle = ();

    fn process_token(&self, token: Token, _line_number: u64) -> TokenSinkResult<Self::Handle> {
        if matches!(
            token,
            Token::DoctypeToken(_) | Token::TagToken(_) | Token::CommentToken(_)
        ) {
            self.0.set(true);
        }
        TokenSinkResult::Continue
    }
}

/// Returns whether bounded UTF-8 input contains authored HTML markup according
/// to the HTML5 tokenizer. Synthesized DOM nodes therefore do not become false signals.
pub fn detect_html(bytes: &[u8], max_input_bytes: u64) -> Result<bool, ConversionError> {
    let actual = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if actual > max_input_bytes {
        return Err(ConversionError::LimitExceeded {
            limit: "input_bytes",
            actual,
            maximum: max_input_bytes,
        });
    }
    let input = std::str::from_utf8(bytes).map_err(|error| ConversionError::CorruptInput {
        message: format!("HTML input is not valid UTF-8: {error}"),
    })?;
    let queue = BufferQueue::default();
    queue.push_back(StrTendril::from_slice(input));
    let tokenizer = Tokenizer::new(HtmlSignalSink(Cell::new(false)), Default::default());
    let _ = tokenizer.feed(&queue);
    tokenizer.end();
    Ok(tokenizer.sink.0.get())
}

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
        convert::document_from_dom_bytes(dom, request)
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
