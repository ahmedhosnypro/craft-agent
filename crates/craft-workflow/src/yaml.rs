//! A tiny YAML-subset parser for workflow frontmatter: block maps, block
//! sequences, block scalars, and flat scalars. Flow-style values live in
//! `yaml_flow`. Every size is capped; every failure is a named parse error.

use crate::yaml_flow::parse_flow;

pub const MAX_YAML_LINES: usize = 1_024;
pub const MAX_YAML_LINE_CHARS: usize = 8_192;
pub(crate) const MAX_DEPTH: usize = 8;
pub(crate) const MAX_MAP_KEYS: usize = 64;
pub(crate) const MAX_SEQ_ITEMS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YValue {
    Map(Vec<(String, YValue)>),
    Seq(Vec<YValue>),
    Str(String),
}

impl YValue {
    pub fn as_map(&self) -> Option<&[(String, YValue)]> {
        if let Self::Map(entries) = self {
            Some(entries)
        } else {
            None
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        if let Self::Str(text) = self {
            Some(text)
        } else {
            None
        }
    }

    pub fn as_seq(&self) -> Option<&[YValue]> {
        if let Self::Seq(items) = self {
            Some(items)
        } else {
            None
        }
    }

    pub fn get(&self, key: &str) -> Option<&YValue> {
        self.as_map()?
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }
}

#[derive(Debug, Clone)]
struct YLine {
    indent: usize,
    text: String,
}

fn tokenize(text: &str) -> Result<Vec<YLine>, String> {
    let mut out = Vec::new();
    for (i, raw) in text.split('\n').enumerate() {
        if out.len() >= MAX_YAML_LINES {
            return Err(format!("more than {MAX_YAML_LINES} lines"));
        }
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        if raw.chars().count() > MAX_YAML_LINE_CHARS {
            return Err(format!(
                "line {} exceeds {MAX_YAML_LINE_CHARS} chars",
                i + 1
            ));
        }
        if raw.starts_with('\t') {
            return Err(format!("line {} uses tab indentation", i + 1));
        }
        let indent = raw.len() - raw.trim_start_matches(' ').len();
        out.push(YLine {
            indent,
            text: raw[indent..].to_string(),
        });
    }
    Ok(out)
}

/// Splits `key: value` or `key:`; the colon must be followed by a space.
fn split_key(text: &str) -> Result<(String, &str), String> {
    let bytes = text.as_bytes();
    let idx = (0..bytes.len())
        .find(|&i| bytes[i] == b':' && (i + 1 >= bytes.len() || bytes[i + 1] == b' '))
        .ok_or_else(|| format!("expected `key: value`, found `{text}`"))?;
    let key = text[..idx].trim();
    if key.is_empty() {
        return Err("empty key".to_string());
    }
    Ok((key.to_string(), &text[idx + 1..]))
}

struct YParser {
    lines: Vec<YLine>,
    pos: usize,
    injected: Option<YLine>,
}

impl YParser {
    fn next_structural(&self) -> Option<usize> {
        (self.pos..self.lines.len())
            .find(|&i| !self.lines[i].text.is_empty() && !self.lines[i].text.starts_with('#'))
    }

    fn peek(&self) -> Option<YLine> {
        if let Some(line) = &self.injected {
            return Some(line.clone());
        }
        self.next_structural().map(|i| self.lines[i].clone())
    }

    fn take(&mut self) {
        if self.injected.is_some() {
            self.injected = None;
            return;
        }
        if let Some(i) = self.next_structural() {
            self.pos = i + 1;
        }
    }

    /// Reads a `|` block scalar: every following line blank or indented
    /// deeper than the key; the first content line fixes the base indent.
    fn block_scalar(&mut self, parent_indent: usize) -> Result<String, String> {
        let mut content: Vec<YLine> = Vec::new();
        while let Some(line) = self.lines.get(self.pos) {
            if line.text.is_empty() || line.indent > parent_indent {
                if line.text.is_empty() {
                    content.push(YLine {
                        indent: 0,
                        text: String::new(),
                    });
                } else {
                    content.push(line.clone());
                }
                self.pos += 1;
            } else {
                break;
            }
        }
        while content.last().is_some_and(|l| l.text.is_empty()) {
            content.pop();
        }
        let base = content
            .iter()
            .filter(|l| !l.text.is_empty())
            .map(|l| l.indent)
            .min()
            .unwrap_or(0);
        let mut out = String::new();
        for (i, line) in content.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            let extra = line.indent.saturating_sub(base);
            for _ in 0..extra {
                out.push(' ');
            }
            out.push_str(&line.text);
        }
        Ok(out)
    }

    fn parse_map(&mut self, indent: usize, depth: usize) -> Result<YValue, String> {
        if depth > MAX_DEPTH {
            return Err(format!("nesting deeper than {MAX_DEPTH} levels"));
        }
        let mut entries: Vec<(String, YValue)> = Vec::new();
        while let Some(line) = self.peek() {
            if line.indent < indent {
                break;
            }
            if line.indent > indent {
                return Err(format!(
                    "line `{}` is indented deeper than its key",
                    line.text
                ));
            }
            if line.text.starts_with("- ") || line.text == "-" {
                return Err("a list item appears where a key is required".to_string());
            }
            if entries.len() >= MAX_MAP_KEYS {
                return Err(format!("more than {MAX_MAP_KEYS} keys in one map"));
            }
            let (key, rest) = split_key(&line.text)?;
            if entries.iter().any(|(k, _)| *k == key) {
                return Err(format!("duplicate key `{key}`"));
            }
            self.take();
            let value = self.parse_value(rest, indent, depth)?;
            entries.push((key, value));
        }
        Ok(YValue::Map(entries))
    }

    fn parse_value(
        &mut self,
        rest: &str,
        key_indent: usize,
        depth: usize,
    ) -> Result<YValue, String> {
        let rest_trim = rest.trim();
        if rest_trim == "|" {
            return Ok(YValue::Str(self.block_scalar(key_indent)?));
        }
        if rest_trim.is_empty() {
            let Some(line) = self.peek() else {
                return Ok(YValue::Map(Vec::new()));
            };
            if line.indent <= key_indent {
                return Ok(YValue::Map(Vec::new()));
            }
            let child = line.indent;
            if line.text.starts_with("- ") || line.text == "-" {
                return self.parse_seq(child, depth + 1);
            }
            return self.parse_map(child, depth + 1);
        }
        parse_flow(rest_trim)
    }

    fn parse_seq(&mut self, indent: usize, depth: usize) -> Result<YValue, String> {
        if depth > MAX_DEPTH {
            return Err(format!("nesting deeper than {MAX_DEPTH} levels"));
        }
        let mut items: Vec<YValue> = Vec::new();
        while let Some(line) = self.peek() {
            if line.indent < indent {
                break;
            }
            if line.indent > indent {
                return Err("unexpected indentation inside a list".to_string());
            }
            if items.len() >= MAX_SEQ_ITEMS {
                return Err(format!("more than {MAX_SEQ_ITEMS} list items"));
            }
            let Some(rest) = line.text.strip_prefix("- ") else {
                return Err(format!("list item expected, found `{}`", line.text));
            };
            if rest.trim().is_empty() {
                return Err("empty list item".to_string());
            }
            self.take();
            let trimmed = rest.trim();
            let is_flow = trimmed.starts_with('{') || trimmed.starts_with('[');
            if !is_flow && split_key(rest).is_ok() {
                self.injected = Some(YLine {
                    indent: indent + 2,
                    text: rest.to_string(),
                });
                items.push(self.parse_map(indent + 2, depth + 1)?);
            } else {
                items.push(parse_flow(trimmed)?);
            }
        }
        Ok(YValue::Seq(items))
    }
}

/// Parses a frontmatter block into one value (usually a map).
pub fn parse(text: &str) -> Result<YValue, String> {
    let lines = tokenize(text)?;
    let mut parser = YParser {
        lines,
        pos: 0,
        injected: None,
    };
    let Some(line) = parser.peek() else {
        return Ok(YValue::Map(Vec::new()));
    };
    let indent = line.indent;
    if line.text.starts_with("- ") || line.text == "-" {
        parser.parse_seq(indent, 0)
    } else {
        parser.parse_map(indent, 0)
    }
}
