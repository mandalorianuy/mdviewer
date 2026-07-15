use std::{fs, path::PathBuf};

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
    let title = find_first_element(&dom.document, "title")
        .map(|node| normalized_text(&node))
        .filter(|value| !value.is_empty());
    let base_url = find_first_element(&dom.document, "base")
        .and_then(|node| attribute(&node, "href"))
        .and_then(|href| resolve_base(&href, request.source_url.as_ref()));
    let canonical_parent = request
        .source
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .canonicalize()
        .map_err(|source| ConversionError::Io {
            path: request.source.clone(),
            source,
        })?;
    let mut converter = DomConverter {
        base_url,
        request,
        canonical_parent,
        assets: Vec::new(),
        warnings: Vec::new(),
        decoded_asset_bytes: 0,
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
    base_url: Option<Url>,
    request: &'a ConversionRequest,
    canonical_parent: PathBuf,
    assets: Vec<Asset>,
    warnings: Vec<ConversionWarning>,
    decoded_asset_bytes: u64,
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
                        text: raw_text(child),
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
                    if contains_element(child, "img") {
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
            } else if contains_element(child, "img") {
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
        let mut inlines = Vec::new();
        for child in node.children.borrow().iter() {
            self.append_inline_node(child, &mut inlines)?;
        }
        normalize_inlines(&mut inlines);
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
                    let content = self.inline_children(node)?;
                    if !inlines_empty(&content) {
                        inlines.push(Inline::Strong(content));
                    }
                }
                Some("em" | "i") => {
                    let content = self.inline_children(node)?;
                    if !inlines_empty(&content) {
                        inlines.push(Inline::Emphasis(content));
                    }
                }
                Some("code") => inlines.push(Inline::Code(raw_text(node))),
                Some("a") => {
                    let content = self.inline_children(node)?;
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
        collect_elements(node, "tr", &mut row_nodes);
        let mut rows = Vec::new();
        let mut alignments = Vec::new();
        for row_node in row_nodes {
            let cells: Vec<_> = row_node
                .children
                .borrow()
                .iter()
                .filter(|cell| matches!(element_name(cell).as_deref(), Some("th" | "td")))
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
        let Some(source) = attribute(node, "src") else {
            self.skipped_image(&alt, "HTML image has no source", blocks);
            return Ok(());
        };
        let Some((data, media_type, extension)) = self.load_image(&source)? else {
            self.skipped_image(
                &alt,
                format!("Skipped external or inaccessible HTML image {source:?}"),
                blocks,
            );
            return Ok(());
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

    fn load_image(
        &self,
        source: &str,
    ) -> Result<Option<(Vec<u8>, String, String)>, ConversionError> {
        let source = source.trim();
        if source.to_ascii_lowercase().starts_with("data:") {
            let Ok(data_url) = DataUrl::process(source) else {
                return Ok(None);
            };
            let mime = data_url.mime_type().to_string();
            let Some(extension) = extension_for_media_type(&mime) else {
                return Ok(None);
            };
            let Ok((data, _fragment)) = data_url.decode_to_vec() else {
                return Ok(None);
            };
            return Ok(Some((data, mime, extension.into())));
        }

        let path = if let Ok(url) = Url::parse(source) {
            if url.scheme() != "file" {
                return Ok(None);
            }
            let Ok(path) = url.to_file_path() else {
                return Ok(None);
            };
            path
        } else {
            let Ok(base) = Url::from_directory_path(&self.canonical_parent) else {
                return Ok(None);
            };
            let Some(path) = base
                .join(source)
                .ok()
                .filter(|url| url.scheme() == "file")
                .and_then(|url| url.to_file_path().ok())
            else {
                return Ok(None);
            };
            path
        };
        let Ok(canonical) = path.canonicalize() else {
            return Ok(None);
        };
        if !canonical.starts_with(&self.canonical_parent) {
            return Ok(None);
        }
        let Ok(metadata) = fs::metadata(&canonical) else {
            return Ok(None);
        };
        if !metadata.is_file() {
            return Ok(None);
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
            return Ok(None);
        };
        let media_type = media_type_for_extension(&extension).to_owned();
        let data = fs::read(&canonical).map_err(|source| ConversionError::Io {
            path: canonical.clone(),
            source,
        })?;
        Ok(Some((data, media_type, extension)))
    }

    fn skipped_image(&mut self, alt: &str, message: impl Into<String>, blocks: &mut Vec<Block>) {
        if !alt.is_empty() {
            blocks.push(Block::Paragraph {
                content: vec![Inline::Text(alt.to_owned())],
            });
        }
        self.warn(WarningCode::ExternalAssetSkipped, message.into());
    }

    fn resolve_link(&self, destination: &str) -> String {
        let destination = destination.trim();
        self.base_url
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
        if attribute(node, "aria-hidden")
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("true"))
        {
            return true;
        }
        attribute(node, "style").is_some_and(|style| style_hides(&style))
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

fn contains_element(node: &Handle, wanted: &str) -> bool {
    node.children.borrow().iter().any(|child| {
        element_name(child).as_deref() == Some(wanted) || contains_element(child, wanted)
    })
}

fn collect_elements(node: &Handle, wanted: &str, found: &mut Vec<Handle>) {
    for child in node.children.borrow().iter() {
        if element_name(child).as_deref() == Some(wanted) {
            found.push(child.clone());
        } else {
            collect_elements(child, wanted, found);
        }
    }
}

fn raw_text(node: &Handle) -> String {
    let mut text = String::new();
    append_raw_text(node, &mut text);
    text
}

fn append_raw_text(node: &Handle, output: &mut String) {
    if let NodeData::Text { contents } = &node.data {
        output.push_str(&contents.borrow());
    }
    for child in node.children.borrow().iter() {
        append_raw_text(child, output);
    }
}

fn normalized_text(node: &Handle) -> String {
    raw_text(node)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_inlines(inlines: &mut Vec<Inline>) {
    let mut previous_space = true;
    normalize_inline_list(inlines, &mut previous_space);
    trim_inline_edges(inlines);
}

fn normalize_inline_list(inlines: &mut Vec<Inline>, previous_space: &mut bool) {
    for inline in inlines.iter_mut() {
        match inline {
            Inline::Text(text) => {
                let mut normalized = String::new();
                for character in text.chars() {
                    if character.is_whitespace() {
                        if !*previous_space {
                            normalized.push(' ');
                            *previous_space = true;
                        }
                    } else {
                        normalized.push(character);
                        *previous_space = false;
                    }
                }
                *text = normalized;
            }
            Inline::Emphasis(content) | Inline::Strong(content) => {
                normalize_inline_list(content, previous_space);
            }
            Inline::Code(_) | Inline::Link { .. } => *previous_space = false,
            Inline::LineBreak => *previous_space = true,
        }
    }
    inlines.retain(|inline| !matches!(inline, Inline::Text(text) if text.is_empty()));
}

fn trim_inline_edges(inlines: &mut Vec<Inline>) {
    if let Some(Inline::Text(text)) = inlines.first_mut() {
        *text = text.trim_start().to_owned();
    }
    if let Some(Inline::Text(text)) = inlines.last_mut() {
        *text = text.trim_end().to_owned();
    }
    inlines.retain(|inline| !matches!(inline, Inline::Text(text) if text.is_empty()));
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

fn resolve_base(href: &str, fallback: Option<&Url>) -> Option<Url> {
    let href = href.trim();
    if is_unsafe_link(href) {
        return None;
    }
    Url::parse(href)
        .ok()
        .or_else(|| fallback.and_then(|base| base.join(href).ok()))
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

fn pre_language(node: &Handle) -> Option<String> {
    let code = find_first_element(node, "code")?;
    attribute(&code, "class")?
        .split_whitespace()
        .find_map(|class| {
            class
                .strip_prefix("language-")
                .filter(|language| !language.is_empty())
                .map(ToOwned::to_owned)
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
