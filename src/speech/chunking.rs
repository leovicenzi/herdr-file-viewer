//! Bounded speech phrases with a byte-exact map back to displayed lines.
//! Markdown soft line breaks become spaces for synthesis, never document edits.

use ratatui::{
    style::{Color, Modifier},
    text::Line,
};
use serde::Serialize;

const MAX_CHARS: usize = 240;
pub const MAX_BYTES: usize = 256 * 1024;
pub const MAX_CHUNKS: usize = 2048;
// Bound text indexing separately from the rolling audio requests. Default previews
// are smaller; even configured large previews cannot make a speech index unbounded.
const INDEX_BYTES: usize = 4 * 1024 * 1024;
const INDEX_CHUNKS: usize = 32768;

#[derive(Clone, Copy)]
pub enum TextMode {
    Plain,
    Markdown,
    MarkdownSource,
}

pub fn index(
    lines: &[Line<'_>],
    first: usize,
    gutters: &[usize],
    mode: TextMode,
) -> Vec<SpeechChunk> {
    build(
        lines,
        first,
        gutters,
        !matches!(mode, TextMode::Plain),
        matches!(mode, TextMode::MarkdownSource),
        INDEX_BYTES,
        INDEX_CHUNKS,
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextRange {
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

/// An exact run copied into speech text. Synthetic join spaces have no screen range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpeechPart {
    pub text_start: usize,
    pub range: TextRange,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct SpeechChunk {
    pub text: String,
    // Retain the original protocol's first-line metadata for external helpers.
    pub line: usize,
    pub start: usize,
    pub end: usize,
    /// Local-only source map; backend progress offsets remain relative to `text`.
    #[serde(skip)]
    pub parts: Vec<SpeechPart>,
    /// Display-derived paragraph/block anchor, local to this settled preview.
    #[serde(skip)]
    pub block: usize,
}

/// First phrase containing a display caret, or the next readable phrase after it.
pub fn chunk_at(chunks: &[SpeechChunk], line: usize, byte: usize) -> Option<usize> {
    chunks.iter().position(|chunk| {
        if chunk.parts.is_empty() {
            chunk.line > line || (chunk.line == line && chunk.end > byte)
        } else {
            chunk
                .parts
                .iter()
                .any(|p| p.range.line > line || (p.range.line == line && p.range.end > byte))
        }
    })
}

impl SpeechChunk {
    pub fn ranges(&self, start: usize, end: usize) -> Vec<TextRange> {
        if start >= end
            || end > self.text.len()
            || !self.text.is_char_boundary(start)
            || !self.text.is_char_boundary(end)
        {
            return Vec::new();
        }
        // Compatibility with an injected, single-line phrase without an explicit map.
        if self.parts.is_empty() {
            return vec![TextRange {
                line: self.line,
                start: self.start + start,
                end: self.start + end,
            }];
        }
        self.parts
            .iter()
            .filter_map(|part| {
                let lo = start.max(part.text_start);
                let hi = end.min(part.text_start + part.range.end - part.range.start);
                (lo < hi).then(|| TextRange {
                    line: part.range.line,
                    start: part.range.start + lo - part.text_start,
                    end: part.range.start + hi - part.text_start,
                })
            })
            .collect()
    }
}

/// Non-Markdown keeps the existing line-oriented reading behavior.
pub fn chunks(lines: &[Line<'_>], first: usize, gutters: &[usize]) -> Vec<SpeechChunk> {
    build(lines, first, gutters, false, false, MAX_BYTES, MAX_CHUNKS)
}

/// Join soft-wrapped Markdown prose before sentence splitting; keep block boundaries.
pub fn markdown_chunks(lines: &[Line<'_>], first: usize, gutters: &[usize]) -> Vec<SpeechChunk> {
    build(lines, first, gutters, true, false, MAX_BYTES, MAX_CHUNKS)
}

/// Source Markdown can distinguish explicit hard breaks from renderer padding.
pub fn markdown_source_chunks(
    lines: &[Line<'_>],
    first: usize,
    gutters: &[usize],
) -> Vec<SpeechChunk> {
    build(lines, first, gutters, true, true, MAX_BYTES, MAX_CHUNKS)
}

#[derive(Default)]
struct Block {
    text: String,
    parts: Vec<SpeechPart>,
    anchor: usize,
}

impl Block {
    fn append(&mut self, text: &str, line: usize, start: usize) {
        if !self.text.is_empty() {
            self.text.push(' ');
        }
        self.parts.push(SpeechPart {
            text_start: self.text.len(),
            range: TextRange {
                line,
                start,
                end: start + text.len(),
            },
        });
        self.text.push_str(text);
    }

    fn flush(
        &mut self,
        result: &mut Vec<SpeechChunk>,
        bytes: &mut usize,
        max_bytes: usize,
        max_chunks: usize,
    ) {
        let block = self.anchor;
        let mut offset = 0;
        while offset < self.text.len() && result.len() < max_chunks && *bytes < max_bytes {
            let tail = &self.text[offset..];
            offset += tail.len() - tail.trim_start().len();
            if offset == self.text.len() {
                break;
            }
            let tail = &self.text[offset..];
            let limit = tail
                .char_indices()
                .nth(MAX_CHARS)
                .map_or(tail.len(), |(i, _)| i);
            let sentence_end = tail[..limit].char_indices().find_map(|(i, ch)| {
                let after = i + ch.len_utf8();
                (matches!(ch, '.' | '!' | '?' | '。' | '！' | '？')
                    && tail[after..].chars().next().is_none_or(char::is_whitespace))
                .then_some(after)
            });
            let end = sentence_end.unwrap_or_else(|| {
                if limit == tail.len() {
                    return limit;
                }
                // An oversized sentence should pause at its last available clause,
                // keeping the comma in the synthesis text for natural intonation.
                let comma = tail[..limit].char_indices().rev().find_map(|(i, ch)| {
                    let after = i + ch.len_utf8();
                    let clause = matches!(ch, '，' | '、')
                        || (ch == ','
                            && tail[after..].chars().next().is_none_or(char::is_whitespace));
                    (clause && tail[..i].chars().any(char::is_alphanumeric)).then_some(after)
                });
                comma.unwrap_or_else(|| {
                    tail[..limit]
                        .char_indices()
                        .rev()
                        .find(|&(i, c)| i > 0 && c.is_whitespace())
                        .map_or(limit, |(i, _)| i)
                })
            });
            let text = tail[..end].trim_end();
            if text.chars().any(char::is_alphanumeric) {
                if *bytes + text.len() > max_bytes {
                    break;
                }
                let first_part = self
                    .parts
                    .partition_point(|p| p.text_start + p.range.end - p.range.start <= offset);
                let parts: Vec<_> = self.parts[first_part..]
                    .iter()
                    .take_while(|p| p.text_start < offset + text.len())
                    .filter_map(|part| {
                        let lo = offset.max(part.text_start);
                        let hi = (offset + text.len())
                            .min(part.text_start + part.range.end - part.range.start);
                        (lo < hi).then(|| SpeechPart {
                            text_start: lo - offset,
                            range: TextRange {
                                line: part.range.line,
                                start: part.range.start + lo - part.text_start,
                                end: part.range.start + hi - part.text_start,
                            },
                        })
                    })
                    .collect();
                if let Some(first) = parts.first() {
                    result.push(SpeechChunk {
                        text: text.into(),
                        line: first.range.line,
                        start: first.range.start,
                        end: first.range.end,
                        parts,
                        block,
                    });
                    *bytes += text.len();
                }
            }
            offset += end;
        }
        self.text.clear();
        self.parts.clear();
    }
}

fn list_start(text: &str) -> bool {
    if ["- ", "* ", "+ ", "• ", "‣ ", "◦ ", "▪ "]
        .iter()
        .any(|s| text.starts_with(s))
    {
        return true;
    }
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    digits > 0 && (text[digits..].starts_with(". ") || text[digits..].starts_with(") "))
}

fn heading(text: &str, styled: &Line<'_>) -> bool {
    let hashes = text.bytes().take_while(|c| *c == b'#').count();
    if (1..=6).contains(&hashes) && text[hashes..].starts_with(' ') {
        return true;
    }
    // Glow removes '#' but keeps the whole heading bold. A partial emphasis/link
    // inside a prose line is not a heading and must not split a sentence.
    let mut visible = styled
        .spans
        .iter()
        .filter(|s| !s.content.trim().is_empty())
        .peekable();
    visible.peek().is_some()
        && visible.all(|s| {
            styled
                .style
                .patch(s.style)
                .add_modifier
                .contains(Modifier::BOLD)
        })
}

fn fence(text: &str) -> Option<(u8, usize)> {
    let marker = *text.as_bytes().first()?;
    if !matches!(marker, b'`' | b'~') {
        return None;
    }
    let count = text.bytes().take_while(|&c| c == marker).count();
    (count >= 3).then_some((marker, count))
}

fn build(
    lines: &[Line<'_>],
    first: usize,
    gutters: &[usize],
    markdown: bool,
    source: bool,
    max_bytes: usize,
    max_chunks: usize,
) -> Vec<SpeechChunk> {
    let mut result = Vec::new();
    let mut bytes = 0;
    let mut block = Block::default();
    let mut fenced: Option<(u8, usize)> = None;
    let mut previous_quote = false;
    let mut buffered = 0;
    let mut plain_block = None;
    for (line, styled) in lines.iter().enumerate() {
        let plain: String = styled.spans.iter().map(|s| s.content.as_ref()).collect();
        let gutter = plain
            .char_indices()
            .nth(gutters.get(line).copied().unwrap_or(0))
            .map_or(plain.len(), |(i, _)| i);
        let body = &plain[gutter..];
        let trimmed = body.trim();
        // Scan fences before the starting viewport too, so starting mid-code never
        // accidentally joins code lines as prose. Closing fences must match the opener.
        let marker = markdown.then(|| fence(body.trim_start())).flatten();
        let in_code = fenced.is_some() || marker.is_some();
        if let Some((ch, count)) = marker {
            match fenced {
                Some((open, width))
                    if ch == open
                        && count >= width
                        && body.trim_start()[count..].trim().is_empty() =>
                {
                    fenced = None
                }
                None => fenced = Some((ch, count)),
                _ => {}
            }
        }
        if line < first {
            continue;
        }
        if bytes + buffered >= max_bytes || result.len() >= max_chunks {
            break;
        }
        if trimmed.is_empty() || !trimmed.chars().any(char::is_alphanumeric) {
            block.flush(&mut result, &mut bytes, max_bytes, max_chunks);
            buffered = 0;
            plain_block = None;
            continue;
        }
        let offset = gutter + body.len() - body.trim_start().len();
        let quote = trimmed.starts_with('>') || trimmed.starts_with('│');
        let table =
            trimmed.contains('│') || trimmed.contains('┃') || trimmed.matches('|').count() >= 2;
        // Rendered fenced code may have no visible fences/indent (Glow's palette
        // style). Conservatively isolate multicolored syntax rather than joining it.
        let mut syntax_colors = Vec::new();
        if markdown && !source {
            for span in &styled.spans {
                if !span.content.trim().is_empty()
                    && let Some(color @ (Color::Indexed(16..) | Color::Rgb(..))) = span.style.fg
                    && !syntax_colors.contains(&color)
                {
                    syntax_colors.push(color);
                }
            }
        }
        let isolated = !markdown
            || in_code
            || table
            || heading(trimmed, styled)
            || syntax_colors.len() >= 2
            || body.chars().take_while(|c| *c == ' ').count() >= 4;
        if isolated || list_start(trimmed) || quote != previous_quote {
            block.flush(&mut result, &mut bytes, max_bytes, max_chunks);
            buffered = 0;
        }
        // Bound paragraph construction as well as final requests, including one huge line.
        let available =
            max_bytes.saturating_sub(bytes + buffered + usize::from(!block.text.is_empty()));
        let mut length = trimmed.len().min(available);
        while !trimmed.is_char_boundary(length) {
            length -= 1;
        }
        if length == 0 {
            break;
        }
        if block.text.is_empty() {
            block.anchor = if markdown {
                line
            } else {
                *plain_block.get_or_insert(line)
            };
        }
        block.append(&trimmed[..length], line, offset);
        buffered = block.text.len();
        previous_quote = quote;
        // Renderers pad to pane width: trailing spaces there are NOT Markdown
        // hard-break syntax. Only interpret them when reading actual source.
        let hard_break = source
            && (body.ends_with("  ")
                || (body.ends_with('\\')
                    && body.chars().rev().take_while(|c| *c == '\\').count() % 2 == 1));
        if isolated || hard_break || trimmed.ends_with(':') {
            block.flush(&mut result, &mut bytes, max_bytes, max_chunks);
            buffered = 0;
        }
    }
    block.flush(&mut result, &mut bytes, max_bytes, max_chunks);
    result
}
