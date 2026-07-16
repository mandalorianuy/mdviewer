use thiserror::Error;
use url::Url;

use crate::jobs::{JobError, PrintJobId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("deep link is invalid")]
pub struct DeepLinkError;

pub fn parse_print_deep_link(value: &str) -> Result<PrintJobId, DeepLinkError> {
    let raw_id = value
        .strip_prefix("mdviewer://print/")
        .ok_or(DeepLinkError)?;
    if raw_id.is_empty() || raw_id.contains(['/', '%', '?', '#']) || !raw_id.is_ascii() {
        return Err(DeepLinkError);
    }

    let url = Url::parse(value).map_err(|_| DeepLinkError)?;
    if url.scheme() != "mdviewer"
        || url.host_str() != Some("print")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != format!("/{raw_id}")
    {
        return Err(DeepLinkError);
    }
    PrintJobId::parse(raw_id).map_err(|_: JobError| DeepLinkError)
}
