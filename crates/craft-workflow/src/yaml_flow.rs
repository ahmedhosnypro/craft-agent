//! Flow-style values: `[a, b]` lists and `{k: v}` maps on one line.

use crate::yaml::{MAX_DEPTH, MAX_MAP_KEYS, MAX_SEQ_ITEMS, YValue};

pub(crate) fn parse_flow(text: &str) -> Result<YValue, String> {
    let mut cur = 0;
    let value = flow_value(text, &mut cur, 0)?;
    let rest = text[cur..].trim();
    if !rest.is_empty() && !rest.starts_with('#') {
        return Err(format!("trailing content after value: `{rest}`"));
    }
    Ok(value)
}

fn skip_ws(text: &str, cur: &mut usize) {
    while text.as_bytes().get(*cur) == Some(&b' ') {
        *cur += 1;
    }
}

fn flow_value(text: &str, cur: &mut usize, depth: usize) -> Result<YValue, String> {
    if depth > MAX_DEPTH {
        return Err(format!("nesting deeper than {MAX_DEPTH} levels"));
    }
    match text.as_bytes().get(*cur) {
        Some(b'[') => {
            *cur += 1;
            let mut items = Vec::new();
            skip_ws(text, cur);
            if text.as_bytes().get(*cur) == Some(&b']') {
                *cur += 1;
                return Ok(YValue::Seq(items));
            }
            loop {
                if items.len() >= MAX_SEQ_ITEMS {
                    return Err(format!("more than {MAX_SEQ_ITEMS} list items"));
                }
                items.push(flow_value(text, cur, depth + 1)?);
                skip_ws(text, cur);
                match text.as_bytes().get(*cur) {
                    Some(b',') => *cur += 1,
                    Some(b']') => {
                        *cur += 1;
                        return Ok(YValue::Seq(items));
                    }
                    _ => return Err("expected `,` or `]` in list".to_string()),
                }
                skip_ws(text, cur);
            }
        }
        Some(b'{') => {
            *cur += 1;
            let mut entries = Vec::new();
            skip_ws(text, cur);
            if text.as_bytes().get(*cur) == Some(&b'}') {
                *cur += 1;
                return Ok(YValue::Map(entries));
            }
            loop {
                if entries.len() >= MAX_MAP_KEYS {
                    return Err(format!("more than {MAX_MAP_KEYS} keys in one map"));
                }
                let key = flow_key(text, cur)?;
                if text.as_bytes().get(*cur) != Some(&b':') {
                    return Err(format!("expected `:` after key `{key}`"));
                }
                *cur += 1;
                skip_ws(text, cur);
                let value = flow_value(text, cur, depth + 1)?;
                entries.push((key, value));
                skip_ws(text, cur);
                match text.as_bytes().get(*cur) {
                    Some(b',') => *cur += 1,
                    Some(b'}') => {
                        *cur += 1;
                        return Ok(YValue::Map(entries));
                    }
                    _ => return Err("expected `,` or `}` in map".to_string()),
                }
                skip_ws(text, cur);
            }
        }
        _ => Ok(YValue::Str(flow_scalar(text, cur)?)),
    }
}

fn flow_key(text: &str, cur: &mut usize) -> Result<String, String> {
    let end = text[*cur..]
        .find([':', ',', '}'])
        .map(|rel| *cur + rel)
        .unwrap_or(text.len());
    let key = text[*cur..end].trim();
    if key.is_empty() {
        return Err("empty key in flow map".to_string());
    }
    *cur = end;
    Ok(key.to_string())
}

fn flow_scalar(text: &str, cur: &mut usize) -> Result<String, String> {
    let bytes = text.as_bytes();
    if bytes.get(*cur) == Some(&b'"') || bytes.get(*cur) == Some(&b'\'') {
        let quote = bytes[*cur];
        let start = *cur + 1;
        let end = text[start..]
            .find(quote as char)
            .map(|rel| start + rel)
            .ok_or_else(|| "unterminated quote".to_string())?;
        let inner = text[start..end].to_string();
        *cur = end + 1;
        return Ok(inner);
    }
    let mut end = *cur;
    while let Some(&b) = bytes.get(end) {
        if b == b',' || b == b']' || b == b'}' {
            break;
        }
        end += 1;
    }
    let raw = text[*cur..end].trim();
    let cut = raw.find(" #").unwrap_or(raw.len());
    *cur = end;
    Ok(raw[..cut].trim_end().to_string())
}
