use std::{collections::HashMap, fs, path::PathBuf};

use data_url::DataUrl;
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use mdconvert_core::{
    Alignment, Asset, AssetId, Block, ConversionError, ConversionRequest, ConversionWarning,
    Document, DocumentMetadata, Inline, ListItem, WarningCode,
};
use url::Url;

pub(crate) fn document_from_dom(
    dom: RcDom,
    request: &ConversionRequest,
) -> Result<Document, ConversionError> {
    document_from_dom_with_asset_refs(dom, request, &HashMap::new())
}

pub(crate) fn document_from_dom_with_asset_refs(
    dom: RcDom,
    request: &ConversionRequest,
    asset_refs: &HashMap<String, AssetId>,
) -> Result<Document, ConversionError> {
    let title = find_first_element(&dom.document, "title")
        .map(|node| normalized_unfiltered_text(&node))
        .filter(|value| !value.is_empty());
    let canonical_source = request
        .source
        .canonicalize()
        .map_err(|source| ConversionError::Io {
            path: request.source.clone(),
            source,
        })?;
    let canonical_parent = canonical_source
        .parent()
        .expect("a canonical file path has a parent")
        .to_owned();
    let input_file_url =
        Url::from_file_path(&canonical_source).map_err(|()| ConversionError::ConversionFailed {
            message: format!(
                "could not represent HTML input as a file URL: {}",
                canonical_source.display()
            ),
        })?;
    let document_base = find_first_element_with_nonempty_attribute(&dom.document, "base", "href")
        .and_then(|href| resolve_base(&href, request.source_url.as_ref(), &input_file_url));
    let effective_base = document_base.or_else(|| request.source_url.clone());
    let mut converter = DomConverter {
        effective_base,
        request,
        canonical_parent,
        assets: Vec::new(),
        warnings: Vec::new(),
        decoded_asset_bytes: 0,
        asset_refs,
    };
    let mut blocks = Vec::new();
    converter.append_container_blocks(&dom.document, &mut blocks)?;

    Ok(Document {
        metadata: DocumentMetadata {
            title,
            source_format: Some("html".into()),
            ..DocumentMetadata::default()
        },
        blocks,
        assets: converter.assets,
        warnings: converter.warnings,
    })
}

struct DomConverter<'a> {
    effective_base: Option<Url>,
    request: &'a ConversionRequest,
    canonical_parent: PathBuf,
    assets: Vec<Asset>,
    warnings: Vec<ConversionWarning>,
    decoded_asset_bytes: u64,
    asset_refs: &'a HashMap<String, AssetId>,
}

enum ImageLoad {
    Loaded {
        data: Vec<u8>,
        media_type: String,
        extension: String,
    },
    Skipped {
        kind: SkippedImageKind,
        reason: String,
    },
}

impl ImageLoad {
    fn invalid(reason: String) -> Self {
        Self::Skipped {
            kind: SkippedImageKind::Invalid,
            reason,
        }
    }

    fn external(reason: String) -> Self {
        Self::Skipped {
            kind: SkippedImageKind::External,
            reason,
        }
    }
}

enum SkippedImageKind {
    Invalid,
    External,
}

impl SkippedImageKind {
    fn warning_code(&self) -> WarningCode {
        match self {
            Self::Invalid => WarningCode::InvalidAssetSkipped,
            Self::External => WarningCode::ExternalAssetSkipped,
        }
    }
}

impl DomConverter<'_> {
    fn append_container_blocks(
        &mut self,
        node: &Handle,
        blocks: &mut Vec<Block>,
    ) -> Result<(), ConversionError> {
        let mut pending = Vec::new();
        for child in node.children.borrow().iter() {
            if self.is_invisible(child) {
                continue;
            }
            match element_name(child).as_deref() {
                Some("h1" | "h2" | "h3" | "h4" | "h5" | "h6") => {
                    flush_paragraph(&mut pending, blocks);
                    let level = element_name(child)
                        .and_then(|name| name[1..].parse::<u8>().ok())
                        .expect("matched heading level");
                    let content = self.inline_children(child)?;
                    if !inlines_empty(&content) {
                        blocks.push(Block::heading(level, content)?);
                    }
                }
                Some("p") => {
                    flush_paragraph(&mut pending, blocks);
                    self.append_mixed_content(child, blocks)?;
                }
                Some("ul" | "ol") => {
                    flush_paragraph(&mut pending, blocks);
                    blocks.push(self.convert_list(child)?);
                }
                Some("table") => {
                    flush_paragraph(&mut pending, blocks);
                    if let Some(table) = self.convert_table(child)? {
                        blocks.push(table);
                    }
                }
                Some("blockquote") => {
                    flush_paragraph(&mut pending, blocks);
                    let mut quote_blocks = Vec::new();
                    self.append_container_blocks(child, &mut quote_blocks)?;
                    if !quote_blocks.is_empty() {
                        blocks.push(Block::Quote {
                            blocks: quote_blocks,
                        });
                    }
                }
                Some("pre") => {
                    flush_paragraph(&mut pending, blocks);
                    let language = pre_language(child);
                    blocks.push(Block::Code {
                        language,
                        text: visible_raw_text(child),
                    });
                }
                Some("hr") => {
                    flush_paragraph(&mut pending, blocks);
                    blocks.push(Block::ThematicBreak);
                }
                Some("img") => {
                    flush_paragraph(&mut pending, blocks);
                    self.append_image(child, blocks)?;
                }
                Some(name) if is_inline_element(name) => {
                    if contains_visible_element(child, "img") {
                        flush_paragraph(&mut pending, blocks);
                        self.append_mixed_content(child, blocks)?;
                    } else {
                        self.append_inline_node(child, &mut pending)?;
                    }
                }
                Some(_) => {
                    flush_paragraph(&mut pending, blocks);
                    self.append_container_blocks(child, blocks)?;
                }
                None => self.append_inline_node(child, &mut pending)?,
            }
        }
        flush_paragraph(&mut pending, blocks);
        Ok(())
    }

    fn append_mixed_content(
        &mut self,
        node: &Handle,
        blocks: &mut Vec<Block>,
    ) -> Result<(), ConversionError> {
        let mut pending = Vec::new();
        for child in node.children.borrow().iter() {
            if self.is_invisible(child) {
                continue;
            }
            if element_name(child).as_deref() == Some("img") {
                flush_paragraph(&mut pending, blocks);
                self.append_image(child, blocks)?;
            } else if contains_visible_element(child, "img") {
                flush_paragraph(&mut pending, blocks);
                self.append_mixed_content(child, blocks)?;
            } else {
                self.append_inline_node(child, &mut pending)?;
            }
        }
        flush_paragraph(&mut pending, blocks);
        Ok(())
    }

    fn inline_children(&mut self, node: &Handle) -> Result<Vec<Inline>, ConversionError> {
        let mut inlines = self.raw_inline_children(node)?;
        normalize_inlines(&mut inlines);
        Ok(inlines)
    }

    fn raw_inline_children(&mut self, node: &Handle) -> Result<Vec<Inline>, ConversionError> {
        let mut inlines = Vec::new();
        for child in node.children.borrow().iter() {
            self.append_inline_node(child, &mut inlines)?;
        }
        Ok(inlines)
    }

    fn append_inline_node(
        &mut self,
        node: &Handle,
        inlines: &mut Vec<Inline>,
    ) -> Result<(), ConversionError> {
        if self.is_invisible(node) {
            return Ok(());
        }
        match &node.data {
            NodeData::Text { contents } => {
                inlines.push(Inline::Text(contents.borrow().to_string()));
            }
            NodeData::Element { .. } => match element_name(node).as_deref() {
                Some("br") => inlines.push(Inline::LineBreak),
                Some("strong" | "b") => {
                    let content = self.raw_inline_children(node)?;
                    if !inlines_empty(&content) {
                        inlines.push(Inline::Strong(content));
                    }
                }
                Some("em" | "i") => {
                    let content = self.raw_inline_children(node)?;
                    if !inlines_empty(&content) {
                        inlines.push(Inline::Emphasis(content));
                    }
                }
                Some("code") => inlines.push(Inline::Code(visible_raw_text(node))),
                Some("a") => {
                    let content = self.raw_inline_children(node)?;
                    if let Some(destination) = attribute(node, "href") {
                        if is_unsafe_link(&destination) {
                            inlines.extend(content);
                            self.warn(
                                WarningCode::InvalidLinkSkipped,
                                format!("Skipped unsafe HTML link destination {destination:?}"),
                            );
                        } else {
                            let url = self.resolve_link(&destination);
                            inlines.push(Inline::Link {
                                url,
                                title: attribute(node, "title")
                                    .map(|value| value.trim().to_owned())
                                    .filter(|value| !value.is_empty()),
                                content,
                            });
                        }
                    } else {
                        inlines.extend(content);
                    }
                }
                Some("img") => {
                    let alt = attribute(node, "alt").unwrap_or_default();
                    if !alt.trim().is_empty() {
                        inlines.push(Inline::Text(alt));
                    }
                }
                Some(_) => {
                    for child in node.children.borrow().iter() {
                        self.append_inline_node(child, inlines)?;
                    }
                }
                None => {}
            },
            _ => {}
        }
        Ok(())
    }

    fn convert_list(&mut self, node: &Handle) -> Result<Block, ConversionError> {
        let ordered = element_name(node).as_deref() == Some("ol");
        let start = ordered
            .then(|| attribute(node, "start").and_then(|value| value.trim().parse().ok()))
            .flatten();
        let mut items = Vec::new();
        for child in node.children.borrow().iter() {
            if element_name(child).as_deref() != Some("li") || self.is_invisible(child) {
                continue;
            }
            let mut blocks = Vec::new();
            self.append_container_blocks(child, &mut blocks)?;
            if !blocks.is_empty() {
                items.push(ListItem { blocks });
            }
        }
        Ok(Block::List {
            ordered,
            start,
            items,
        })
    }

    fn convert_table(&mut self, node: &Handle) -> Result<Option<Block>, ConversionError> {
        let mut row_nodes = Vec::new();
        collect_visible_elements(node, "tr", &mut row_nodes);
        let mut rows = Vec::new();
        let mut alignments = Vec::new();
        for row_node in row_nodes {
            let cells: Vec<_> = row_node
                .children
                .borrow()
                .iter()
                .filter(|cell| {
                    !is_invisible_node(cell)
                        && matches!(element_name(cell).as_deref(), Some("th" | "td"))
                })
                .cloned()
                .collect();
            if cells.is_empty() {
                continue;
            }
            if alignments.is_empty() {
                alignments = cells.iter().map(cell_alignment).collect();
            }
            let mut row = Vec::new();
            for cell in cells {
                row.push(self.inline_children(&cell)?);
            }
            rows.push(row);
        }
        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
        if width == 0 {
            return Ok(None);
        }
        for row in &mut rows {
            row.resize_with(width, Vec::new);
        }
        alignments.resize(width, Alignment::None);
        Ok(Some(Block::Table { alignments, rows }))
    }

    fn append_image(
        &mut self,
        node: &Handle,
        blocks: &mut Vec<Block>,
    ) -> Result<(), ConversionError> {
        let alt = attribute(node, "alt").unwrap_or_default().trim().to_owned();
        if alt.is_empty() {
            self.warn(
                WarningCode::MissingImageAlt,
                "HTML image has no nonempty alt text".into(),
            );
        }
        let Some(source) = attribute(node, "src").filter(|source| !source.trim().is_empty()) else {
            self.skipped_image(
                &alt,
                WarningCode::InvalidAssetSkipped,
                "HTML image has no nonempty source",
                blocks,
            );
            return Ok(());
        };
        if let Some(asset_id) = self.asset_refs.get(source.trim()) {
            blocks.push(Block::Image {
                asset_id: asset_id.clone(),
                alt,
            });
            return Ok(());
        }
        let (data, media_type, extension) = match self.load_image(&source)? {
            ImageLoad::Loaded {
                data,
                media_type,
                extension,
            } => (data, media_type, extension),
            ImageLoad::Skipped { kind, reason } => {
                self.skipped_image(&alt, kind.warning_code(), reason, blocks);
                return Ok(());
            }
        };

        let next = self.assets.len() as u64 + 1;
        if next > u64::from(self.request.limits.max_assets) {
            return Err(ConversionError::LimitExceeded {
                limit: "assets",
                actual: next,
                maximum: u64::from(self.request.limits.max_assets),
            });
        }
        let data_len = u64::try_from(data.len()).unwrap_or(u64::MAX);
        let total = self.decoded_asset_bytes.saturating_add(data_len);
        if total > self.request.limits.max_input_bytes {
            return Err(ConversionError::LimitExceeded {
                limit: "asset_bytes",
                actual: total,
                maximum: self.request.limits.max_input_bytes,
            });
        }
        self.decoded_asset_bytes = total;
        let id = AssetId::new(format!("html-image-{next:03}"))?;
        self.assets.push(Asset {
            id: id.clone(),
            file_name: format!("image-{next:03}.{extension}"),
            media_type,
            data,
        });
        blocks.push(Block::Image { asset_id: id, alt });
        Ok(())
    }

    fn load_image(&self, source: &str) -> Result<ImageLoad, ConversionError> {
        let source = source.trim();
        if source.to_ascii_lowercase().starts_with("data:") {
            let Ok(data_url) = DataUrl::process(source) else {
                return Ok(ImageLoad::invalid(format!(
                    "Skipped malformed HTML data image {source:?}"
                )));
            };
            let mime = data_url.mime_type().to_string();
            let Some(extension) = extension_for_media_type(&mime) else {
                return Ok(ImageLoad::invalid(format!(
                    "Skipped unsupported HTML data image type {mime:?}"
                )));
            };
            let Ok((data, _fragment)) = data_url.decode_to_vec() else {
                return Ok(ImageLoad::invalid(
                    "Skipped malformed HTML data image payload".into(),
                ));
            };
            return Ok(ImageLoad::Loaded {
                data,
                media_type: mime,
                extension: extension.into(),
            });
        }

        let path = if let Ok(url) = Url::parse(source) {
            if url.scheme() != "file" {
                return Ok(ImageLoad::external(format!(
                    "Skipped external HTML image {source:?}"
                )));
            }
            let Ok(path) = url.to_file_path() else {
                return Ok(ImageLoad::invalid(format!(
                    "Skipped invalid HTML file image URL {source:?}"
                )));
            };
            path
        } else {
            if source.starts_with("//") {
                return Ok(ImageLoad::external(format!(
                    "Skipped external HTML image {source:?}"
                )));
            }
            let base = if let Some(base) = &self.effective_base {
                base.clone()
            } else {
                let Ok(base) = Url::from_directory_path(&self.canonical_parent) else {
                    return Ok(ImageLoad::invalid(
                        "Skipped image because its input directory is not a valid file URL".into(),
                    ));
                };
                base
            };
            let Ok(resolved) = base.join(source) else {
                return Ok(ImageLoad::invalid(format!(
                    "Skipped invalid relative HTML image source {source:?}"
                )));
            };
            if resolved.scheme() != "file" {
                return Ok(ImageLoad::external(format!(
                    "Skipped external HTML image {}",
                    resolved
                )));
            }
            let Ok(path) = resolved.to_file_path() else {
                return Ok(ImageLoad::invalid(format!(
                    "Skipped invalid HTML file image URL {resolved}"
                )));
            };
            path
        };
        let Ok(canonical) = path.canonicalize() else {
            return Ok(ImageLoad::invalid(format!(
                "Skipped inaccessible local HTML image {}",
                path.display()
            )));
        };
        if !canonical.starts_with(&self.canonical_parent) {
            return Ok(ImageLoad::external(format!(
                "Skipped out-of-root local HTML image {}",
                canonical.display()
            )));
        }
        let Ok(metadata) = fs::metadata(&canonical) else {
            return Ok(ImageLoad::invalid(format!(
                "Skipped inaccessible local HTML image {}",
                canonical.display()
            )));
        };
        if !metadata.is_file() {
            return Ok(ImageLoad::invalid(format!(
                "Skipped non-file local HTML image {}",
                canonical.display()
            )));
        }
        let total = self.decoded_asset_bytes.saturating_add(metadata.len());
        if total > self.request.limits.max_input_bytes {
            return Err(ConversionError::LimitExceeded {
                limit: "asset_bytes",
                actual: total,
                maximum: self.request.limits.max_input_bytes,
            });
        }
        let Some(extension) = canonical
            .extension()
            .and_then(|value| value.to_str())
            .and_then(safe_image_extension)
            .map(ToOwned::to_owned)
        else {
            return Ok(ImageLoad::invalid(format!(
                "Skipped unsupported local HTML image type {}",
                canonical.display()
            )));
        };
        let media_type = media_type_for_extension(&extension).to_owned();
        let Ok(data) = fs::read(&canonical) else {
            return Ok(ImageLoad::invalid(format!(
                "Skipped unreadable local HTML image {}",
                canonical.display()
            )));
        };
        Ok(ImageLoad::Loaded {
            data,
            media_type,
            extension,
        })
    }

    fn skipped_image(
        &mut self,
        alt: &str,
        code: WarningCode,
        message: impl Into<String>,
        blocks: &mut Vec<Block>,
    ) {
        if !alt.is_empty() {
            blocks.push(Block::Paragraph {
                content: vec![Inline::Text(alt.to_owned())],
            });
        }
        self.warn(code, message.into());
    }

    fn resolve_link(&self, destination: &str) -> String {
        let destination = destination.trim();
        self.effective_base
            .as_ref()
            .and_then(|base| base.join(destination).ok())
            .or_else(|| {
                self.request
                    .source_url
                    .as_ref()
                    .and_then(|base| base.join(destination).ok())
            })
            .map_or_else(|| destination.to_owned(), |url| url.to_string())
    }

    fn is_invisible(&self, node: &Handle) -> bool {
        is_invisible_node(node)
    }

    fn warn(&mut self, code: WarningCode, message: String) {
        self.warnings.push(ConversionWarning {
            code,
            message,
            page: None,
        });
    }
}

fn element_name(node: &Handle) -> Option<String> {
    match &node.data {
        NodeData::Element { name, .. } => Some(name.local.to_string()),
        _ => None,
    }
}

fn attribute(node: &Handle, wanted: &str) -> Option<String> {
    let NodeData::Element { attrs, .. } = &node.data else {
        return None;
    };
    attrs
        .borrow()
        .iter()
        .find(|attribute| attribute.name.local.as_ref() == wanted)
        .map(|attribute| attribute.value.to_string())
}

fn has_attribute(node: &Handle, wanted: &str) -> bool {
    let NodeData::Element { attrs, .. } = &node.data else {
        return false;
    };
    attrs
        .borrow()
        .iter()
        .any(|attribute| attribute.name.local.as_ref() == wanted)
}

fn find_first_element(node: &Handle, wanted: &str) -> Option<Handle> {
    if element_name(node).as_deref() == Some(wanted) {
        return Some(node.clone());
    }
    node.children
        .borrow()
        .iter()
        .find_map(|child| find_first_element(child, wanted))
}

fn find_first_element_with_nonempty_attribute(
    node: &Handle,
    wanted_element: &str,
    wanted_attribute: &str,
) -> Option<String> {
    if element_name(node).as_deref() == Some(wanted_element)
        && let Some(value) = attribute(node, wanted_attribute)
        && !value.trim().is_empty()
    {
        return Some(value);
    }
    node.children.borrow().iter().find_map(|child| {
        find_first_element_with_nonempty_attribute(child, wanted_element, wanted_attribute)
    })
}

fn contains_visible_element(node: &Handle, wanted: &str) -> bool {
    if is_invisible_node(node) {
        return false;
    }
    node.children.borrow().iter().any(|child| {
        !is_invisible_node(child)
            && (element_name(child).as_deref() == Some(wanted)
                || contains_visible_element(child, wanted))
    })
}

fn collect_visible_elements(node: &Handle, wanted: &str, found: &mut Vec<Handle>) {
    for child in node.children.borrow().iter() {
        if is_invisible_node(child) {
            continue;
        }
        if element_name(child).as_deref() == Some(wanted) {
            found.push(child.clone());
        } else {
            collect_visible_elements(child, wanted, found);
        }
    }
}

fn unfiltered_raw_text(node: &Handle) -> String {
    let mut text = String::new();
    append_unfiltered_raw_text(node, &mut text);
    text
}

fn append_unfiltered_raw_text(node: &Handle, output: &mut String) {
    if let NodeData::Text { contents } = &node.data {
        output.push_str(&contents.borrow());
    }
    for child in node.children.borrow().iter() {
        append_unfiltered_raw_text(child, output);
    }
}

fn visible_raw_text(node: &Handle) -> String {
    let mut text = String::new();
    append_visible_raw_text(node, &mut text);
    text
}

fn append_visible_raw_text(node: &Handle, output: &mut String) {
    if let NodeData::Text { contents } = &node.data {
        output.push_str(&contents.borrow());
    }
    for child in node.children.borrow().iter() {
        if !is_invisible_node(child) {
            append_visible_raw_text(child, output);
        }
    }
}

fn normalized_unfiltered_text(node: &Handle) -> String {
    unfiltered_raw_text(node)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_inlines(inlines: &mut Vec<Inline>) {
    *inlines = normalize_inline_sequence(std::mem::take(inlines)).content;
}

struct NormalizedInlineSequence {
    content: Vec<Inline>,
    leading_space: bool,
    trailing_space: bool,
}

struct InlineNormalizer {
    content: Vec<Inline>,
    leading_space: bool,
    pending_space: bool,
}

impl InlineNormalizer {
    fn new() -> Self {
        Self {
            content: Vec::new(),
            leading_space: false,
            pending_space: false,
        }
    }

    fn mark_space(&mut self) {
        if self.content.is_empty() {
            self.leading_space = true;
        } else if !matches!(self.content.last(), Some(Inline::LineBreak)) {
            self.pending_space = true;
        }
    }

    fn push_visible(&mut self, inline: Inline) {
        if self.pending_space && !self.content.is_empty() {
            self.content.push(Inline::Text(" ".into()));
        }
        self.pending_space = false;
        self.content.push(inline);
    }

    fn push_line_break(&mut self) {
        self.pending_space = false;
        self.content.push(Inline::LineBreak);
    }

    fn finish(self) -> NormalizedInlineSequence {
        NormalizedInlineSequence {
            content: self.content,
            leading_space: self.leading_space,
            trailing_space: self.pending_space,
        }
    }
}

fn normalize_inline_sequence(inlines: Vec<Inline>) -> NormalizedInlineSequence {
    let mut normalizer = InlineNormalizer::new();
    for inline in inlines {
        match inline {
            Inline::Text(text) => {
                let mut run = String::new();
                for character in text.chars() {
                    if is_html_whitespace(character) {
                        if !run.is_empty() {
                            normalizer.push_visible(Inline::Text(std::mem::take(&mut run)));
                        }
                        normalizer.mark_space();
                    } else {
                        run.push(character);
                    }
                }
                if !run.is_empty() {
                    normalizer.push_visible(Inline::Text(run));
                }
            }
            Inline::Emphasis(content) => {
                push_normalized_container(&mut normalizer, content, Inline::Emphasis);
            }
            Inline::Strong(content) => {
                push_normalized_container(&mut normalizer, content, Inline::Strong);
            }
            Inline::Link {
                url,
                title,
                content,
            } => {
                let nested = normalize_inline_sequence(content);
                if nested.leading_space {
                    normalizer.mark_space();
                }
                if !nested.content.is_empty() {
                    normalizer.push_visible(Inline::Link {
                        url,
                        title,
                        content: nested.content,
                    });
                }
                if nested.trailing_space {
                    normalizer.mark_space();
                }
            }
            Inline::Code(text) => {
                let leading_space = text.chars().next().is_some_and(is_html_whitespace);
                let trailing_space = text.chars().next_back().is_some_and(is_html_whitespace);
                if leading_space {
                    normalizer.mark_space();
                }
                let text = text.trim_matches(is_html_whitespace);
                if !text.is_empty() {
                    normalizer.push_visible(Inline::Code(text.to_owned()));
                }
                if trailing_space {
                    normalizer.mark_space();
                }
            }
            Inline::LineBreak => normalizer.push_line_break(),
        }
    }
    normalizer.finish()
}

fn push_normalized_container(
    normalizer: &mut InlineNormalizer,
    content: Vec<Inline>,
    wrap: impl FnOnce(Vec<Inline>) -> Inline,
) {
    let nested = normalize_inline_sequence(content);
    if nested.leading_space {
        normalizer.mark_space();
    }
    if !nested.content.is_empty() {
        normalizer.push_visible(wrap(nested.content));
    }
    if nested.trailing_space {
        normalizer.mark_space();
    }
}

fn is_html_whitespace(character: char) -> bool {
    matches!(character, '\t' | '\n' | '\u{000c}' | '\r' | ' ')
}

fn flush_paragraph(inlines: &mut Vec<Inline>, blocks: &mut Vec<Block>) {
    normalize_inlines(inlines);
    if !inlines_empty(inlines) {
        blocks.push(Block::Paragraph {
            content: std::mem::take(inlines),
        });
    } else {
        inlines.clear();
    }
}

fn inlines_empty(inlines: &[Inline]) -> bool {
    inlines.iter().all(|inline| match inline {
        Inline::Text(text) | Inline::Code(text) => text.is_empty(),
        Inline::Emphasis(content) | Inline::Strong(content) => inlines_empty(content),
        Inline::Link { content, .. } => inlines_empty(content),
        Inline::LineBreak => false,
    })
}

fn is_inline_element(name: &str) -> bool {
    matches!(
        name,
        "a" | "abbr"
            | "b"
            | "br"
            | "cite"
            | "code"
            | "del"
            | "em"
            | "i"
            | "ins"
            | "kbd"
            | "mark"
            | "q"
            | "s"
            | "small"
            | "span"
            | "strong"
            | "sub"
            | "sup"
            | "time"
            | "u"
            | "var"
    )
}

fn resolve_base(href: &str, request_base: Option<&Url>, input_file_url: &Url) -> Option<Url> {
    let href = href.trim();
    if is_unsafe_link(href) {
        return None;
    }
    if let Ok(url) = Url::parse(href) {
        return (!url.cannot_be_a_base()).then_some(url);
    }
    request_base
        .and_then(|base| base.join(href).ok())
        .or_else(|| input_file_url.join(href).ok())
        .filter(|url| !url.cannot_be_a_base())
}

fn is_unsafe_link(destination: &str) -> bool {
    let trimmed = destination.trim_start();
    let scheme = trimmed.split_once(':').map(|(scheme, _)| {
        scheme
            .chars()
            .filter(|character| !character.is_ascii_whitespace() && !character.is_control())
            .collect::<String>()
            .to_ascii_lowercase()
    });
    matches!(scheme.as_deref(), Some("javascript" | "data"))
}

fn style_hides(style: &str) -> bool {
    style.split(';').any(|declaration| {
        let Some((property, value)) = declaration.split_once(':') else {
            return false;
        };
        let property = property.trim();
        let value = value.trim().to_ascii_lowercase();
        let value = value.strip_suffix("!important").unwrap_or(&value).trim();
        property.eq_ignore_ascii_case("display") && value == "none"
            || property.eq_ignore_ascii_case("visibility") && value == "hidden"
    })
}

fn is_invisible_node(node: &Handle) -> bool {
    let Some(name) = element_name(node) else {
        return false;
    };
    if matches!(
        name.as_str(),
        "head" | "script" | "style" | "template" | "noscript"
    ) {
        return true;
    }
    if has_attribute(node, "hidden") {
        return true;
    }
    if attribute(node, "aria-hidden").is_some_and(|value| value.trim().eq_ignore_ascii_case("true"))
    {
        return true;
    }
    attribute(node, "style").is_some_and(|style| style_hides(&style))
}

fn pre_language(node: &Handle) -> Option<String> {
    let code = find_first_visible_element(node, "code")?;
    attribute(&code, "class")?
        .split_whitespace()
        .find_map(|class| {
            class
                .strip_prefix("language-")
                .filter(|language| !language.is_empty())
                .map(ToOwned::to_owned)
        })
}

fn find_first_visible_element(node: &Handle, wanted: &str) -> Option<Handle> {
    node.children.borrow().iter().find_map(|child| {
        if is_invisible_node(child) {
            None
        } else if element_name(child).as_deref() == Some(wanted) {
            Some(child.clone())
        } else {
            find_first_visible_element(child, wanted)
        }
    })
}

fn cell_alignment(node: &Handle) -> Alignment {
    let value = attribute(node, "align").or_else(|| {
        attribute(node, "style").and_then(|style| {
            style.split(';').find_map(|declaration| {
                let (property, value) = declaration.split_once(':')?;
                property
                    .trim()
                    .eq_ignore_ascii_case("text-align")
                    .then(|| value.trim().to_owned())
            })
        })
    });
    match value.as_deref().map(str::trim) {
        Some(value) if value.eq_ignore_ascii_case("left") => Alignment::Left,
        Some(value) if value.eq_ignore_ascii_case("center") => Alignment::Center,
        Some(value) if value.eq_ignore_ascii_case("right") => Alignment::Right,
        _ => Alignment::None,
    }
}

fn extension_for_media_type(media_type: &str) -> Option<&'static str> {
    match media_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "image/bmp" => Some("bmp"),
        "image/svg+xml" => Some("svg"),
        "image/avif" => Some("avif"),
        _ => None,
    }
}

fn safe_image_extension(extension: &str) -> Option<&str> {
    match extension.to_ascii_lowercase().as_str() {
        "png" => Some("png"),
        "jpg" | "jpeg" => Some("jpg"),
        "gif" => Some("gif"),
        "webp" => Some("webp"),
        "bmp" => Some("bmp"),
        "svg" => Some("svg"),
        "avif" => Some("avif"),
        _ => None,
    }
}

fn media_type_for_extension(extension: &str) -> &'static str {
    match extension {
        "png" => "image/png",
        "jpg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "avif" => "image/avif",
        _ => "application/octet-stream",
    }
}
