use std::collections::HashMap;

use crate::{Alignment, Block, Document, EmitError, Inline, ListItem};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GfmOptions {
    pub final_newline: bool,
}

pub fn emit_gfm(document: &Document, options: &GfmOptions) -> Result<String, EmitError> {
    let assets = AssetIndex::new(document)?;
    let mut markdown = render_blocks(&document.blocks, &assets)?;
    markdown = normalize_line_endings(&markdown);

    while markdown.ends_with('\n') {
        markdown.pop();
    }
    if options.final_newline && !markdown.is_empty() {
        markdown.push('\n');
    }

    Ok(markdown)
}

struct AssetIndex<'a> {
    file_names: HashMap<&'a str, &'a str>,
}

impl<'a> AssetIndex<'a> {
    fn new(document: &'a Document) -> Result<Self, EmitError> {
        let mut file_names = HashMap::with_capacity(document.assets.len());
        for asset in &document.assets {
            if file_names
                .insert(asset.id.as_str(), asset.file_name.as_str())
                .is_some()
            {
                return Err(EmitError::DuplicateAssetId {
                    asset_id: asset.id.as_str().to_owned(),
                });
            }
        }

        Ok(Self { file_names })
    }

    fn file_name(&self, asset_id: &str) -> Result<&'a str, EmitError> {
        self.file_names
            .get(asset_id)
            .copied()
            .ok_or_else(|| EmitError::MissingAsset {
                asset_id: asset_id.to_owned(),
            })
    }
}

fn render_blocks(blocks: &[Block], assets: &AssetIndex<'_>) -> Result<String, EmitError> {
    blocks
        .iter()
        .map(|block| render_block(block, assets))
        .collect::<Result<Vec<_>, _>>()
        .map(|rendered| rendered.join("\n\n"))
}

fn render_block(block: &Block, assets: &AssetIndex<'_>) -> Result<String, EmitError> {
    match block {
        Block::Heading { level, content } => Ok(format!(
            "{} {}",
            "#".repeat(usize::from(*level)),
            render_inlines(content, InlineContext::Text)
        )),
        Block::Paragraph { content } => Ok(render_inlines(content, InlineContext::Text)),
        Block::List {
            ordered,
            start,
            items,
        } => render_list(*ordered, *start, items, assets),
        Block::Table { alignments, rows } => render_table(alignments, rows),
        Block::Code { language, text } => render_code_block(language.as_deref(), text),
        Block::Quote { blocks } => {
            let quote = render_blocks(blocks, assets)?;
            Ok(quote
                .split('\n')
                .map(|line| {
                    if line.is_empty() {
                        ">".to_owned()
                    } else {
                        format!("> {line}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"))
        }
        Block::Image { asset_id, alt } => {
            let file_name = assets.file_name(asset_id.as_str())?;
            Ok(format!(
                "![{}]({})",
                escape_text(alt, InlineContext::Text),
                escape_destination(file_name, InlineContext::Text)
            ))
        }
        Block::ThematicBreak => Ok("***".into()),
    }
}

fn render_list(
    ordered: bool,
    start: Option<u64>,
    items: &[ListItem],
    assets: &AssetIndex<'_>,
) -> Result<String, EmitError> {
    let first_number = start.unwrap_or(1);
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let marker = if ordered {
                format!("{}.", first_number.saturating_add(index as u64))
            } else {
                "-".into()
            };
            let indentation = " ".repeat(marker.len() + 1);
            let body = render_blocks(&item.blocks, assets)?;
            let mut lines = body.split('\n');
            let first_line = lines.next().unwrap_or_default();
            let mut rendered = format!("{marker} {first_line}");

            for line in lines {
                rendered.push('\n');
                if !line.is_empty() {
                    rendered.push_str(&indentation);
                    rendered.push_str(line);
                }
            }

            Ok(rendered)
        })
        .collect::<Result<Vec<_>, EmitError>>()
        .map(|items| items.join("\n"))
}

fn render_table(alignments: &[Alignment], rows: &[Vec<Vec<Inline>>]) -> Result<String, EmitError> {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    if !alignments.is_empty() && alignments.len() != width {
        return Err(EmitError::TableAlignmentWidthMismatch {
            expected: width,
            actual: alignments.len(),
        });
    }
    if width == 0 || rows.is_empty() {
        return Ok(String::new());
    }

    let synthesized;
    let alignments = if alignments.is_empty() {
        synthesized = vec![Alignment::None; width];
        &synthesized
    } else {
        alignments
    };

    let mut lines = Vec::with_capacity(rows.len() + 1);
    lines.push(render_table_row(&rows[0], width));
    lines.push(format!(
        "| {} |",
        alignments
            .iter()
            .map(|alignment| match alignment {
                Alignment::None => "---",
                Alignment::Left => ":---",
                Alignment::Center => ":---:",
                Alignment::Right => "---:",
            })
            .collect::<Vec<_>>()
            .join(" | ")
    ));
    lines.extend(rows[1..].iter().map(|row| render_table_row(row, width)));

    Ok(lines.join("\n"))
}

fn render_table_row(row: &[Vec<Inline>], width: usize) -> String {
    let cells = (0..width)
        .map(|index| {
            row.get(index)
                .map(|cell| render_inlines(cell, InlineContext::Table))
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" | ");
    format!("| {cells} |")
}

fn render_code_block(language: Option<&str>, text: &str) -> Result<String, EmitError> {
    if language.is_some_and(|language| language.contains(['\r', '\n'])) {
        return Err(EmitError::InvalidCodeLanguage);
    }

    let text = normalize_line_endings(text);
    let fence_character =
        if language.is_some_and(|language| !language.is_empty() && language.contains('`')) {
            '~'
        } else {
            '`'
        };
    let fence = fence_character
        .to_string()
        .repeat(3.max(longest_fence_run(&text, fence_character) + 1));
    let language = language.unwrap_or_default();
    let mut rendered = format!("{fence}{language}\n{text}");
    if !text.ends_with('\n') {
        rendered.push('\n');
    }
    rendered.push_str(&fence);
    Ok(rendered)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InlineContext {
    Text,
    Table,
}

fn render_inlines(inlines: &[Inline], context: InlineContext) -> String {
    inlines
        .iter()
        .map(|inline| render_inline(inline, context))
        .collect()
}

fn render_inline(inline: &Inline, context: InlineContext) -> String {
    match inline {
        Inline::Text(text) => escape_text(text, context),
        Inline::Emphasis(content) => format!("*{}*", render_inlines(content, context)),
        Inline::Strong(content) => format!("**{}**", render_inlines(content, context)),
        Inline::Code(code) => render_code_span(code, context),
        Inline::Link {
            url,
            title,
            content,
        } => {
            let title = title
                .as_ref()
                .map(|title| format!(" \"{}\"", escape_title(title, context)))
                .unwrap_or_default();
            format!(
                "[{}]({}{title})",
                render_inlines(content, context),
                escape_destination(url, context)
            )
        }
        Inline::LineBreak => match context {
            InlineContext::Text => "  \n".into(),
            InlineContext::Table => "<br>".into(),
        },
    }
}

fn render_code_span(code: &str, context: InlineContext) -> String {
    if code.is_empty() {
        return "<code></code>".into();
    }

    let mut code = normalize_line_endings(code);
    if context == InlineContext::Table {
        code = code.replace('|', "\\|").replace('\n', "<br>");
    }
    let delimiter = "`".repeat(longest_backtick_run(&code) + 1);
    let needs_padding = code.starts_with(['`', ' ']) || code.ends_with(['`', ' ']);
    if needs_padding && !code.chars().all(|character| character == ' ') {
        format!("{delimiter} {code} {delimiter}")
    } else {
        format!("{delimiter}{code}{delimiter}")
    }
}

fn escape_text(text: &str, context: InlineContext) -> String {
    let text = normalize_line_endings(text);
    let mut escaped = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    let mut at_line_start = true;
    let mut leading_spaces = 0;
    let mut line_prefix_is_digits = true;
    let mut line_has_digit = false;

    while let Some(character) = characters.next() {
        let next_is_whitespace = characters.peek().is_some_and(|next| next.is_whitespace());
        let starts_block_marker = at_line_start
            && leading_spaces <= 3
            && matches!(character, '-' | '+')
            && next_is_whitespace;
        let starts_rule_marker = at_line_start
            && leading_spaces <= 3
            && matches!(character, '-' | '=')
            && begins_rule_line(character, characters.clone());
        let ends_ordered_marker = matches!(character, '.' | ')')
            && leading_spaces <= 3
            && line_prefix_is_digits
            && line_has_digit
            && next_is_whitespace;
        if matches!(
            character,
            '\\' | '`' | '*' | '_' | '~' | '[' | ']' | '<' | '>' | '#' | '!'
        ) || (context == InlineContext::Table && character == '|')
            || starts_block_marker
            || starts_rule_marker
            || ends_ordered_marker
        {
            escaped.push('\\');
        }
        if character == '&' {
            escaped.push_str("&amp;");
        } else if context == InlineContext::Table && character == '\n' {
            escaped.push_str("<br>");
        } else {
            escaped.push(character);
        }

        if character == '\n' {
            at_line_start = true;
            leading_spaces = 0;
            line_prefix_is_digits = true;
            line_has_digit = false;
        } else if at_line_start && character == ' ' {
            leading_spaces += 1;
        } else {
            at_line_start = false;
            if line_prefix_is_digits && character.is_ascii_digit() {
                line_has_digit = true;
            } else {
                line_prefix_is_digits = false;
            }
        }
    }
    escaped
}

fn begins_rule_line(marker: char, mut remaining: std::iter::Peekable<std::str::Chars<'_>>) -> bool {
    let mut run_length = 1;
    while remaining.peek() == Some(&marker) {
        remaining.next();
        run_length += 1;
    }

    run_length >= 3
        && remaining
            .take_while(|character| *character != '\n')
            .all(char::is_whitespace)
}

fn escape_destination(destination: &str, context: InlineContext) -> String {
    let mut escaped = String::with_capacity(destination.len());
    for character in destination.chars() {
        if character.is_ascii_control() || character == ' ' {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            let byte = character as u8;
            escaped.push('%');
            escaped.push(HEX[usize::from(byte >> 4)] as char);
            escaped.push(HEX[usize::from(byte & 0x0f)] as char);
        } else {
            if matches!(character, '\\' | '(' | ')')
                || (context == InlineContext::Table && character == '|')
            {
                escaped.push('\\');
            }
            escaped.push(character);
        }
    }
    escaped
}

fn escape_title(title: &str, context: InlineContext) -> String {
    let title = normalize_line_endings(title);
    let mut escaped = String::with_capacity(title.len());
    for character in title.chars() {
        if matches!(character, '\\' | '"') || (context == InlineContext::Table && character == '|')
        {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn normalize_line_endings(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

fn longest_backtick_run(value: &str) -> usize {
    longest_fence_run(value, '`')
}

fn longest_fence_run(value: &str, fence_character: char) -> usize {
    let mut longest = 0;
    let mut current = 0;
    for character in value.chars() {
        if character == fence_character {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    longest
}
