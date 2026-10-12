//! Line-ending classification and the pure-CRLF edit model: pure-CRLF
//! files are matched on a collapsed LF view and written back through a
//! collapse-then-expand materialization, so a CR can never double.
//! Mixed and lone-CR files are refused by edit_file; LF files keep the
//! byte-exact path.

use std::borrow::Cow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endings {
    Lf,
    Crlf,
    Mixed,
}

/// Classify a file's line endings in one scan: Crlf only when at least
/// one CRLF pair exists and no lone CR and no bare LF does; Mixed on a
/// lone CR or a CRLF/bare-LF mix; Lf otherwise.
pub fn classify(bytes: &[u8]) -> Endings {
    let mut crlf = 0usize;
    let mut lone_cr = 0usize;
    let mut bare_lf = 0usize;
    let mut idx = 0usize;
    while idx < bytes.len() {
        match bytes[idx] {
            b'\r' => {
                if bytes.get(idx + 1) == Some(&b'\n') {
                    crlf += 1;
                    idx += 2;
                } else {
                    lone_cr += 1;
                    idx += 1;
                }
            }
            b'\n' => {
                bare_lf += 1;
                idx += 1;
            }
            _ => idx += 1,
        }
    }
    if lone_cr > 0 || (crlf > 0 && bare_lf > 0) {
        return Endings::Mixed;
    }
    if crlf > 0 { Endings::Crlf } else { Endings::Lf }
}

/// The LF-space view of `bytes` under `style`: Crlf collapses every
/// CRLF pair, leaving lone CRs alone; other styles pass through raw.
pub fn view(bytes: &[u8], style: Endings) -> Cow<'_, [u8]> {
    match style {
        Endings::Crlf => Cow::Owned(collapse(bytes)),
        _ => Cow::Borrowed(bytes),
    }
}

/// Write-back for a spliced view: Crlf collapses any CRLF pair the
/// replacement may carry, then expands every LF back to CRLF —
/// idempotent, so materialize(materialize(x)) == materialize(x).
/// Other styles write the view raw.
pub fn materialize(view: &[u8], style: Endings) -> Vec<u8> {
    match style {
        Endings::Crlf => expand(&collapse(view)),
        _ => view.to_vec(),
    }
}

fn collapse(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut idx = 0usize;
    while idx < bytes.len() {
        if bytes[idx] == b'\r' && bytes.get(idx + 1) == Some(&b'\n') {
            out.push(b'\n');
            idx += 2;
        } else {
            out.push(bytes[idx]);
            idx += 1;
        }
    }
    out
}

fn expand(bytes: &[u8]) -> Vec<u8> {
    let breaks = bytes.iter().filter(|b| **b == b'\n').count();
    let mut out = Vec::with_capacity(bytes.len() + breaks);
    for &b in bytes {
        if b == b'\n' {
            out.push(b'\r');
            out.push(b'\n');
        } else {
            out.push(b);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Endings, classify, materialize, view};

    #[test]
    fn classification_shapes() {
        assert_eq!(classify(b"a\nb"), Endings::Lf);
        assert_eq!(classify(b"no breaks"), Endings::Lf);
        assert_eq!(classify(b"a\r\nb"), Endings::Crlf);
        assert_eq!(classify(b"a\r\nb\r\n"), Endings::Crlf);
        assert_eq!(classify(b"a\r\nb\nc"), Endings::Mixed);
        assert_eq!(classify(b"a\rb"), Endings::Mixed);
        assert_eq!(classify(b"\r\r\n"), Endings::Mixed);
    }

    #[test]
    fn view_collapses_pairs_only() {
        assert_eq!(view(b"a\r\nb", Endings::Crlf).as_ref(), b"a\nb");
        assert_eq!(view(b"a\nb", Endings::Lf).as_ref(), b"a\nb");
    }

    #[test]
    fn materialize_is_idempotent_and_never_doubles() {
        for view_bytes in ["a\nb\n", "x\r\ny\n", "z\rw\n"] {
            let once = materialize(view_bytes.as_bytes(), Endings::Crlf);
            let twice = materialize(&once, Endings::Crlf);
            assert_eq!(once, twice, "{view_bytes:?}");
            assert!(
                !once.windows(2).any(|w| w == b"\r\r"),
                "no doubled CR in {:?}",
                once
            );
        }
        assert_eq!(materialize(b"a\nb", Endings::Lf), b"a\nb".to_vec());
    }
}
