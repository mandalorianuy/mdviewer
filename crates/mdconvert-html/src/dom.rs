use std::{fs, path::Path};

use html5ever::{parse_document, tendril::TendrilSink};
use markup5ever_rcdom::{Handle, RcDom};
use mdconvert_core::ConversionError;

pub(crate) const MAX_DOM_DEPTH: u64 = 256;
pub(crate) const MAX_DOM_NODES: u64 = 1_000_000;

pub(crate) fn parse_file(path: &Path, max_input_bytes: u64) -> Result<RcDom, ConversionError> {
    let metadata = fs::metadata(path).map_err(|source| ConversionError::Io {
        path: path.to_owned(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(ConversionError::CorruptInput {
            message: format!("HTML input is not a regular file: {}", path.display()),
        });
    }
    if metadata.len() > max_input_bytes {
        return Err(ConversionError::LimitExceeded {
            limit: "input_bytes",
            actual: metadata.len(),
            maximum: max_input_bytes,
        });
    }

    let bytes = fs::read(path).map_err(|source| ConversionError::Io {
        path: path.to_owned(),
        source,
    })?;
    let actual = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if actual > max_input_bytes {
        return Err(ConversionError::LimitExceeded {
            limit: "input_bytes",
            actual,
            maximum: max_input_bytes,
        });
    }
    let input = std::str::from_utf8(&bytes).map_err(|error| ConversionError::CorruptInput {
        message: format!("HTML input is not valid UTF-8: {error}"),
    })?;

    let dom = parse_document(RcDom::default(), Default::default()).one(input);
    validate_dom_budget(&dom.document, MAX_DOM_DEPTH, MAX_DOM_NODES)?;
    Ok(dom)
}

pub(crate) fn parse_bytes(bytes: &[u8], max_input_bytes: u64) -> Result<RcDom, ConversionError> {
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
    let dom = parse_document(RcDom::default(), Default::default()).one(input);
    validate_dom_budget(&dom.document, MAX_DOM_DEPTH, MAX_DOM_NODES)?;
    Ok(dom)
}

fn validate_dom_budget(
    root: &Handle,
    max_depth: u64,
    max_nodes: u64,
) -> Result<(), ConversionError> {
    let mut stack = vec![(root.clone(), 0_u64)];
    let mut nodes = 0_u64;
    while let Some((node, depth)) = stack.pop() {
        if depth > max_depth {
            return Err(ConversionError::LimitExceeded {
                limit: "html_dom_depth",
                actual: depth,
                maximum: max_depth,
            });
        }
        nodes = nodes.saturating_add(1);
        if nodes > max_nodes {
            return Err(ConversionError::LimitExceeded {
                limit: "html_dom_nodes",
                actual: nodes,
                maximum: max_nodes,
            });
        }
        stack.extend(
            node.children
                .borrow()
                .iter()
                .rev()
                .map(|child| (child.clone(), depth.saturating_add(1))),
        );
    }
    Ok(())
}

#[cfg(test)]
fn validate_dom_budget_for_test(dom: &RcDom, max_nodes: u64) -> Result<(), ConversionError> {
    validate_dom_budget(&dom.document, MAX_DOM_DEPTH, max_nodes)
}

#[cfg(test)]
mod tests {
    use html5ever::{parse_document, tendril::TendrilSink};
    use markup5ever_rcdom::RcDom;
    use mdconvert_core::ConversionError;

    use super::validate_dom_budget_for_test;

    #[test]
    fn node_budget_reports_the_first_exact_excess_node() {
        let dom = parse_document(RcDom::default(), Default::default()).one("<p>x</p>");

        assert!(matches!(
            validate_dom_budget_for_test(&dom, 3),
            Err(ConversionError::LimitExceeded {
                limit: "html_dom_nodes",
                actual: 4,
                maximum: 3,
            })
        ));
    }
}
