//! Character decoding for documents and stylesheets (WHATWG Encoding; HTML
//! §13.2.3 "determining the character encoding", simplified).

use encoding_rs::{Encoding, UTF_8, WINDOWS_1252};

/// Decoded text and the encoding that was used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedText {
    pub text: String,
    pub encoding: &'static str,
    /// True if malformed sequences were replaced with U+FFFD.
    pub had_errors: bool,
}

fn decode_with(bytes: &[u8], encoding: &'static Encoding) -> DecodedText {
    // `decode` also strips a BOM of the given encoding.
    let (text, used, had_errors) = encoding.decode(bytes);
    DecodedText {
        text: text.into_owned(),
        encoding: used.name(),
        had_errors,
    }
}

/// Looks for `<meta charset=...>` or `<meta http-equiv content="...charset=...">`
/// in the first 1024 bytes (the HTML "prescan", simplified).
fn prescan_meta_charset(bytes: &[u8]) -> Option<&'static Encoding> {
    let head = &bytes[..bytes.len().min(1024)];
    let lower: Vec<u8> = head.iter().map(u8::to_ascii_lowercase).collect();
    let mut pos = 0;
    while let Some(off) = find(&lower[pos..], b"<meta") {
        let start = pos + off;
        let end = lower[start..]
            .iter()
            .position(|&b| b == b'>')
            .map_or(lower.len(), |e| start + e);
        let tag = &lower[start..end];
        if let Some(c) = find(tag, b"charset") {
            let rest = &tag[c + 7..];
            let rest = rest.strip_prefix(b" ").unwrap_or(rest);
            if let Some(value) = rest.strip_prefix(b"=") {
                let value: Vec<u8> = value
                    .iter()
                    .copied()
                    .skip_while(|&b| b == b' ' || b == b'"' || b == b'\'')
                    .take_while(|&b| !matches!(b, b'"' | b'\'' | b' ' | b';' | b'/' | b'>'))
                    .collect();
                if let Some(enc) = Encoding::for_label(&value) {
                    // A meta-declared UTF-16 means UTF-8 (the bytes were ASCII).
                    return Some(if enc.name().starts_with("UTF-16") {
                        UTF_8
                    } else {
                        enc
                    });
                }
            }
        }
        pos = end.max(start + 5);
    }
    None
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Decodes an HTML document: BOM, then the HTTP `charset`, then a `<meta>`
/// prescan, then UTF-8 if the bytes are valid UTF-8, else windows-1252 (the
/// usual default for Western locales).
pub fn decode_html(bytes: &[u8], http_charset: Option<&str>) -> DecodedText {
    if let Some((enc, _)) = Encoding::for_bom(bytes) {
        return decode_with(bytes, enc);
    }
    if let Some(enc) = http_charset.and_then(|c| Encoding::for_label(c.as_bytes())) {
        return decode_with(bytes, enc);
    }
    if let Some(enc) = prescan_meta_charset(bytes) {
        return decode_with(bytes, enc);
    }
    if std::str::from_utf8(bytes).is_ok() {
        return decode_with(bytes, UTF_8);
    }
    decode_with(bytes, WINDOWS_1252)
}

/// Decodes a stylesheet or other text: BOM, HTTP `charset`, else UTF-8.
/// (`@charset` is not consulted yet.)
pub fn decode_text(bytes: &[u8], http_charset: Option<&str>) -> DecodedText {
    if let Some((enc, _)) = Encoding::for_bom(bytes) {
        return decode_with(bytes, enc);
    }
    let enc = http_charset
        .and_then(|c| Encoding::for_label(c.as_bytes()))
        .unwrap_or(UTF_8);
    decode_with(bytes, enc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bom_wins() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("héllo".as_bytes());
        let d = decode_html(&bytes, Some("iso-8859-1"));
        assert_eq!((d.text.as_str(), d.encoding), ("héllo", "UTF-8"));
    }

    #[test]
    fn http_charset_then_meta() {
        let latin1 = b"<p>caf\xe9</p>";
        assert_eq!(decode_html(latin1, Some("iso-8859-1")).text, "<p>café</p>");
        let meta = b"<html><head><meta charset=\"windows-1252\"></head><p>caf\xe9";
        let d = decode_html(meta, None);
        assert_eq!(d.encoding, "windows-1252");
        assert!(d.text.ends_with("café"));
        let http_equiv =
            b"<meta http-equiv=Content-Type content='text/html; charset=ISO-8859-2'><p>\xb1";
        assert_eq!(decode_html(http_equiv, None).encoding, "ISO-8859-2");
    }

    #[test]
    fn fallback_is_utf8_when_valid_else_windows_1252() {
        assert_eq!(decode_html("ශ්‍රී".as_bytes(), None).encoding, "UTF-8");
        let d = decode_html(b"caf\xe9", None);
        assert_eq!((d.text.as_str(), d.encoding), ("café", "windows-1252"));
    }

    #[test]
    fn meta_utf16_means_utf8() {
        assert_eq!(
            decode_html(b"<meta charset=utf-16><p>x", None).encoding,
            "UTF-8"
        );
    }

    #[test]
    fn text_defaults_to_utf8_and_reports_errors() {
        let d = decode_text(b"a\xffb", None);
        assert!(d.had_errors);
        assert_eq!(d.text, "a\u{FFFD}b");
    }
}
