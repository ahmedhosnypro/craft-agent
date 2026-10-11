//! Sanitizer: collected text is data under analysis, never instructions.
//! Channels: HTML entities, HTML comments, markdown alt text and link
//! titles, then control/zero-width/bidi characters on the decoded text.

pub fn sanitize(input: &str) -> String {
    let decoded = decode_entities(input);
    let no_comments = strip_html_comments(&decoded);
    let no_alt = strip_image_alt(&no_comments);
    let no_titles = strip_link_titles(&no_alt);
    strip_hostile_chars(&no_titles)
}

const MAX_REF_BYTES: usize = 12;

/// One-pass entity decode: a ref never re-decodes its own output.
fn decode_entities(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&'
            && let Some((name, next)) = entity_ref(input, i + 1)
        {
            match decode_ref(name) {
                Some(ch) => out.push(ch),
                // unknown refs stay literal; valid numeric refs decoding
                // to hostile codepoints are stripped by the char pass
                None => out.push_str(input.get(i..next).unwrap_or("")),
            }
            i = next;
            continue;
        }
        let end = char_end(bytes, i);
        match input.get(i..end) {
            Some(chunk) => out.push_str(chunk),
            None => out.push('\u{FFFD}'),
        }
        i = end;
    }
    out
}

fn char_end(bytes: &[u8], start: usize) -> usize {
    let mut end = start + 1;
    while end < bytes.len() && bytes[end] & 0xC0 == 0x80 {
        end += 1;
    }
    end.min(bytes.len())
}

/// `&name;` at `start`; only ASCII alphanumerics and a leading `#`.
fn entity_ref(input: &str, start: usize) -> Option<(&str, usize)> {
    let bytes = input.as_bytes();
    let mut i = start;
    let mut len = 0;
    while i < bytes.len() && len < MAX_REF_BYTES {
        let b = bytes[i];
        if b == b';' {
            return input.get(start..i).map(|name| (name, i + 1));
        }
        if !(b.is_ascii_alphanumeric() || (b == b'#' && len == 0)) {
            return None;
        }
        i += 1;
        len += 1;
    }
    None
}

fn decode_ref(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        _ => decode_numeric(name),
    }
}

fn decode_numeric(name: &str) -> Option<char> {
    let body = name.strip_prefix('#')?;
    let value = match body.strip_prefix(['x', 'X']) {
        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
        None => body.parse::<u32>().ok()?,
    };
    char::from_u32(value)
}

fn strip_html_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 4..];
        match after.find("-->") {
            Some(end) => rest = &after[end + 3..],
            None => rest = "", // unterminated comment drops the tail
        }
    }
    out.push_str(rest);
    out
}

/// `![alt](url)` loses its alt text; link text is kept.
fn strip_image_alt(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("![") {
        let after = &rest[start + 2..];
        match after.find("](") {
            Some(close) => {
                out.push_str(&rest[..start]);
                out.push_str("![](");
                rest = &after[close + 2..];
            }
            None => {
                out.push_str(&rest[..start + 2]);
                rest = &rest[start + 2..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// `[t](url "title")` loses the title, not the link.
fn strip_link_titles(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(open) = rest.find("](") {
        let after = &rest[open + 2..];
        let consumed = open + 2;
        if let Some(close) = after.find(')')
            && let Some(url) = url_without_title(&after[..close])
        {
            out.push_str(&rest[..open + 2]);
            out.push_str(url);
            out.push(')');
            rest = &after[close + 1..];
            continue;
        }
        out.push_str(&rest[..consumed]);
        rest = &rest[consumed..];
    }
    out.push_str(rest);
    out
}

fn url_without_title(inside: &str) -> Option<&str> {
    let (url, tail) = inside.split_once(char::is_whitespace)?;
    let quote = tail.chars().next()?;
    let is_pair = matches!(quote, '"' | '\'') && tail.len() >= 2 && tail.ends_with(quote);
    if is_pair { Some(url) } else { None }
}

fn strip_hostile_chars(input: &str) -> String {
    input.chars().filter(|c| !is_hostile(*c)).collect()
}

fn is_hostile(c: char) -> bool {
    matches!(c,
        '\u{0}'..='\u{8}'
            | '\u{b}'
            | '\u{c}'
            | '\u{d}'
            | '\u{e}'..='\u{1f}'
            | '\u{7f}'..='\u{9f}'
            | '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'
            | '\u{2066}'..='\u{2069}'
            | '\u{feff}')
}
