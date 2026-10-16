//! HTML tokenizer (WHATWG HTML §13.2.5).
//!
//! Implemented: data, RCDATA, RAWTEXT, script data (without the script
//! *escape* states), PLAINTEXT, tags and attributes, comments (all states),
//! DOCTYPE with public/system identifiers, bogus comments/doctypes, named and
//! numeric character references.
//!
//! Not implemented (documented in docs/compatibility.md):
//! - script data escaped / double-escaped states (`<script><!-- ... -->`);
//! - CDATA sections (only valid in foreign content, which is unsupported);
//! - the full named-reference table (see `entities.rs`).
//!
//! Input is a `&str` (already decoded); newlines are normalized on
//! construction. Consecutive characters are coalesced into one
//! [`Token::Characters`] so text costs one allocation per run, not per char.

use std::collections::VecDeque;

use rustlite_dom::Attribute;

use crate::entities;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Doctype {
    pub name: Option<String>,
    pub public_id: Option<String>,
    pub system_id: Option<String>,
    pub force_quirks: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Tag {
    pub name: String,
    pub attrs: Vec<Attribute>,
    pub self_closing: bool,
}

impl Tag {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| &*a.name == name)
            .map(|a| a.value.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Doctype(Doctype),
    StartTag(Tag),
    EndTag(Tag),
    Comment(String),
    Characters(String),
    Eof,
}

/// Text-content states the tree builder can switch the tokenizer into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextMode {
    Data,
    Rcdata,
    Rawtext,
    ScriptData,
    Plaintext,
}

/// A recoverable parse error. Parsing never fails; errors are informational.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseError {
    pub code: &'static str,
    /// Byte offset into the normalized input.
    pub offset: usize,
}

/// Errors beyond this many are counted but not stored.
const MAX_STORED_ERRORS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Text(TextMode),
    TagOpen,
    EndTagOpen,
    TagName,
    RawLessThan(TextMode),
    RawEndTagOpen(TextMode),
    RawEndTagName(TextMode),
    BeforeAttributeName,
    AttributeName,
    AfterAttributeName,
    BeforeAttributeValue,
    AttributeValueQuoted(char),
    AttributeValueUnquoted,
    AfterAttributeValueQuoted,
    SelfClosingStartTag,
    BogusComment,
    MarkupDeclarationOpen,
    CommentStart,
    CommentStartDash,
    Comment,
    CommentLessThanSign,
    CommentLessThanSignBang,
    CommentLessThanSignBangDash,
    CommentLessThanSignBangDashDash,
    CommentEndDash,
    CommentEnd,
    CommentEndBang,
    Doctype,
    BeforeDoctypeName,
    DoctypeName,
    AfterDoctypeName,
    AfterDoctypePublicKeyword,
    BeforeDoctypePublicIdentifier,
    DoctypePublicIdentifier(char),
    AfterDoctypePublicIdentifier,
    BetweenDoctypePublicAndSystemIdentifiers,
    AfterDoctypeSystemKeyword,
    BeforeDoctypeSystemIdentifier,
    DoctypeSystemIdentifier(char),
    AfterDoctypeSystemIdentifier,
    BogusDoctype,
}

fn is_ws(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\x0C' | ' ')
}

#[derive(Debug)]
pub struct Tokenizer {
    input: String,
    pos: usize,
    state: State,
    text: String,
    pending: VecDeque<Token>,
    tag: Tag,
    tag_is_end: bool,
    attr_name: String,
    attr_value: String,
    attr_active: bool,
    comment: String,
    doctype: Doctype,
    temp: String,
    last_start_tag: Option<String>,
    errors: Vec<ParseError>,
    error_count: usize,
    done: bool,
}

/// Normalizes CRLF and lone CR to LF (§13.2.3.5).
fn normalize_newlines(input: &str) -> String {
    if !input.contains('\r') {
        return input.to_owned();
    }
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    out
}

impl Tokenizer {
    pub fn new(input: &str) -> Self {
        Self {
            input: normalize_newlines(input),
            pos: 0,
            state: State::Text(TextMode::Data),
            text: String::new(),
            pending: VecDeque::new(),
            tag: Tag::default(),
            tag_is_end: false,
            attr_name: String::new(),
            attr_value: String::new(),
            attr_active: false,
            comment: String::new(),
            doctype: Doctype::default(),
            temp: String::new(),
            last_start_tag: None,
            errors: Vec::new(),
            error_count: 0,
            done: false,
        }
    }

    /// Switches the content model; called by the tree builder after start tags
    /// such as `<title>` or `<style>`.
    pub fn set_text_mode(&mut self, mode: TextMode) {
        self.state = State::Text(mode);
    }

    /// Sets the "last start tag" used for appropriate-end-tag checks (needed
    /// when a fragment parser or tree builder enters RCDATA/RAWTEXT itself).
    pub fn set_last_start_tag(&mut self, name: &str) {
        self.last_start_tag = Some(name.to_owned());
    }

    pub fn errors(&self) -> &[ParseError] {
        &self.errors
    }

    pub fn error_count(&self) -> usize {
        self.error_count
    }

    /// Returns the next token. After [`Token::Eof`] it keeps returning `Eof`.
    pub fn next_token(&mut self) -> Token {
        loop {
            if let Some(t) = self.pending.pop_front() {
                return t;
            }
            if self.done {
                return Token::Eof;
            }
            self.step();
        }
    }

    // ----- input helpers ---------------------------------------------------

    fn next_char(&mut self) -> Option<char> {
        let c = self.input[self.pos..].chars().next()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    /// Un-reads `c` (which must be the char just read).
    fn reconsume(&mut self, c: Option<char>) {
        if let Some(c) = c {
            self.pos -= c.len_utf8();
        }
    }

    fn starts_with_ci(&self, s: &str) -> bool {
        self.input
            .get(self.pos..self.pos + s.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(s))
    }

    fn error(&mut self, code: &'static str) {
        self.error_count += 1;
        if self.errors.len() < MAX_STORED_ERRORS {
            self.errors.push(ParseError {
                code,
                offset: self.pos,
            });
        }
    }

    // ----- emission --------------------------------------------------------

    fn flush_text(&mut self) {
        if !self.text.is_empty() {
            let text = std::mem::take(&mut self.text);
            self.pending.push_back(Token::Characters(text));
        }
    }

    fn emit(&mut self, token: Token) {
        self.flush_text();
        self.pending.push_back(token);
    }

    fn start_tag(&mut self, end: bool) {
        self.tag = Tag::default();
        self.tag_is_end = end;
        self.attr_active = false;
    }

    fn start_attr(&mut self) {
        self.finish_attr();
        self.attr_active = true;
    }

    fn finish_attr(&mut self) {
        if !self.attr_active {
            return;
        }
        self.attr_active = false;
        let name = std::mem::take(&mut self.attr_name);
        let value = std::mem::take(&mut self.attr_value);
        if self.tag.attrs.iter().any(|a| *a.name == *name) {
            self.error("duplicate-attribute");
        } else {
            self.tag.attrs.push(Attribute::new(name, value));
        }
    }

    fn emit_tag(&mut self) {
        self.finish_attr();
        let tag = std::mem::take(&mut self.tag);
        if self.tag_is_end {
            if !tag.attrs.is_empty() {
                self.error("end-tag-with-attributes");
            }
            if tag.self_closing {
                self.error("end-tag-with-trailing-solidus");
            }
            self.emit(Token::EndTag(tag));
        } else {
            self.last_start_tag = Some(tag.name.clone());
            self.emit(Token::StartTag(tag));
        }
        self.state = State::Text(TextMode::Data);
    }

    fn emit_comment(&mut self) {
        let c = std::mem::take(&mut self.comment);
        self.emit(Token::Comment(c));
    }

    fn emit_doctype(&mut self) {
        let d = std::mem::take(&mut self.doctype);
        self.emit(Token::Doctype(d));
    }

    fn emit_eof(&mut self) {
        self.flush_text();
        self.pending.push_back(Token::Eof);
        self.done = true;
    }

    fn is_appropriate_end_tag(&self) -> bool {
        self.last_start_tag.as_deref() == Some(self.tag.name.as_str())
    }

    // ----- character references ---------------------------------------------

    /// Consumes a character reference after `&` and returns the text to emit.
    fn char_ref(&mut self, in_attribute: bool) -> String {
        let rest = &self.input[self.pos..];
        match rest.chars().next() {
            Some('#') => self.numeric_char_ref(),
            Some(c) if c.is_ascii_alphanumeric() => {
                let Some(m) = entities::match_named(rest) else {
                    // Unknown name: `&` is literal; the name is consumed as text.
                    return "&".into();
                };
                let next = rest[m.len..].chars().next();
                if in_attribute
                    && !m.has_semicolon
                    && next.is_some_and(|c| c == '=' || c.is_ascii_alphanumeric())
                {
                    // Historical quirk: `?a=1&copy=2` keeps `&copy` literal in attributes.
                    return "&".into();
                }
                if !m.has_semicolon {
                    self.error("missing-semicolon-after-character-reference");
                }
                self.pos += m.len;
                m.value.to_string()
            }
            _ => "&".into(),
        }
    }

    fn numeric_char_ref(&mut self) -> String {
        let start = self.pos;
        self.pos += 1; // '#'
        let hex = matches!(self.input[self.pos..].chars().next(), Some('x' | 'X'));
        if hex {
            self.pos += 1;
        }
        let digits_start = self.pos;
        let mut value: u32 = 0;
        while let Some(c) = self.input[self.pos..].chars().next() {
            let Some(d) = c.to_digit(if hex { 16 } else { 10 }) else {
                break;
            };
            value = value
                .saturating_mul(if hex { 16 } else { 10 })
                .saturating_add(d);
            self.pos += 1;
        }
        if self.pos == digits_start {
            self.error("absence-of-digits-in-numeric-character-reference");
            let literal = self.input[start - 1..self.pos].to_owned();
            return literal;
        }
        if self.input[self.pos..].starts_with(';') {
            self.pos += 1;
        } else {
            self.error("missing-semicolon-after-character-reference");
        }
        let c = match value {
            0 => {
                self.error("null-character-reference");
                '\u{FFFD}'
            }
            v if v > 0x10FFFF => {
                self.error("character-reference-outside-unicode-range");
                '\u{FFFD}'
            }
            0xD800..=0xDFFF => {
                self.error("surrogate-character-reference");
                '\u{FFFD}'
            }
            v => entities::c1_replacement(v)
                .or_else(|| char::from_u32(v))
                .unwrap_or('\u{FFFD}'),
        };
        c.to_string()
    }

    // ----- the state machine ------------------------------------------------

    fn step(&mut self) {
        let c = self.next_char();
        match self.state {
            State::Text(mode) => self.text_state(mode, c),
            State::TagOpen => match c {
                Some('!') => self.state = State::MarkupDeclarationOpen,
                Some('/') => self.state = State::EndTagOpen,
                Some(ch) if ch.is_ascii_alphabetic() => {
                    self.start_tag(false);
                    self.reconsume(c);
                    self.state = State::TagName;
                }
                Some('?') => {
                    self.error("unexpected-question-mark-instead-of-tag-name");
                    self.comment.clear();
                    self.reconsume(c);
                    self.state = State::BogusComment;
                }
                None => {
                    self.error("eof-before-tag-name");
                    self.text.push('<');
                    self.emit_eof();
                }
                Some(_) => {
                    self.error("invalid-first-character-of-tag-name");
                    self.text.push('<');
                    self.reconsume(c);
                    self.state = State::Text(TextMode::Data);
                }
            },
            State::EndTagOpen => match c {
                Some(ch) if ch.is_ascii_alphabetic() => {
                    self.start_tag(true);
                    self.reconsume(c);
                    self.state = State::TagName;
                }
                Some('>') => {
                    self.error("missing-end-tag-name");
                    self.state = State::Text(TextMode::Data);
                }
                None => {
                    self.error("eof-before-tag-name");
                    self.text.push_str("</");
                    self.emit_eof();
                }
                Some(_) => {
                    self.error("invalid-first-character-of-tag-name");
                    self.comment.clear();
                    self.reconsume(c);
                    self.state = State::BogusComment;
                }
            },
            State::TagName => match c {
                Some(ch) if is_ws(ch) => self.state = State::BeforeAttributeName,
                Some('/') => self.state = State::SelfClosingStartTag,
                Some('>') => self.emit_tag(),
                Some('\0') => {
                    self.error("unexpected-null-character");
                    self.tag.name.push('\u{FFFD}');
                }
                Some(ch) => self.tag.name.push(ch.to_ascii_lowercase()),
                None => self.eof_in_tag(),
            },
            State::RawLessThan(mode) => {
                if c == Some('/') {
                    self.temp.clear();
                    self.state = State::RawEndTagOpen(mode);
                } else {
                    self.text.push('<');
                    self.reconsume(c);
                    self.state = State::Text(mode);
                }
            }
            State::RawEndTagOpen(mode) => match c {
                Some(ch) if ch.is_ascii_alphabetic() => {
                    self.start_tag(true);
                    self.reconsume(c);
                    self.state = State::RawEndTagName(mode);
                }
                _ => {
                    self.text.push_str("</");
                    self.reconsume(c);
                    self.state = State::Text(mode);
                }
            },
            State::RawEndTagName(mode) => {
                let appropriate = self.is_appropriate_end_tag();
                match c {
                    Some(ch) if is_ws(ch) && appropriate => {
                        self.state = State::BeforeAttributeName;
                    }
                    Some('/') if appropriate => self.state = State::SelfClosingStartTag,
                    Some('>') if appropriate => self.emit_tag(),
                    Some(ch) if ch.is_ascii_alphabetic() => {
                        self.tag.name.push(ch.to_ascii_lowercase());
                        self.temp.push(ch);
                    }
                    _ => {
                        self.text.push_str("</");
                        let temp = std::mem::take(&mut self.temp);
                        self.text.push_str(&temp);
                        self.reconsume(c);
                        self.state = State::Text(mode);
                    }
                }
            }
            State::BeforeAttributeName => match c {
                Some(ch) if is_ws(ch) => {}
                Some('/' | '>') | None => {
                    self.reconsume(c);
                    self.state = State::AfterAttributeName;
                }
                Some('=') => {
                    self.error("unexpected-equals-sign-before-attribute-name");
                    self.start_attr();
                    self.attr_name.push('=');
                    self.state = State::AttributeName;
                }
                Some(_) => {
                    self.start_attr();
                    self.reconsume(c);
                    self.state = State::AttributeName;
                }
            },
            State::AttributeName => match c {
                Some(ch) if is_ws(ch) => self.state = State::AfterAttributeName,
                Some('/' | '>') | None => {
                    self.reconsume(c);
                    self.state = State::AfterAttributeName;
                }
                Some('=') => self.state = State::BeforeAttributeValue,
                Some('\0') => {
                    self.error("unexpected-null-character");
                    self.attr_name.push('\u{FFFD}');
                }
                Some(ch) => {
                    if matches!(ch, '"' | '\'' | '<') {
                        self.error("unexpected-character-in-attribute-name");
                    }
                    self.attr_name.push(ch.to_ascii_lowercase());
                }
            },
            State::AfterAttributeName => match c {
                Some(ch) if is_ws(ch) => {}
                Some('/') => self.state = State::SelfClosingStartTag,
                Some('=') => self.state = State::BeforeAttributeValue,
                Some('>') => self.emit_tag(),
                None => self.eof_in_tag(),
                Some(_) => {
                    self.start_attr();
                    self.reconsume(c);
                    self.state = State::AttributeName;
                }
            },
            State::BeforeAttributeValue => match c {
                Some(ch) if is_ws(ch) => {}
                Some(q @ ('"' | '\'')) => self.state = State::AttributeValueQuoted(q),
                Some('>') => {
                    self.error("missing-attribute-value");
                    self.emit_tag();
                }
                _ => {
                    self.reconsume(c);
                    self.state = State::AttributeValueUnquoted;
                }
            },
            State::AttributeValueQuoted(quote) => match c {
                Some(ch) if ch == quote => self.state = State::AfterAttributeValueQuoted,
                Some('&') => {
                    let s = self.char_ref(true);
                    self.attr_value.push_str(&s);
                }
                Some('\0') => {
                    self.error("unexpected-null-character");
                    self.attr_value.push('\u{FFFD}');
                }
                Some(ch) => self.attr_value.push(ch),
                None => self.eof_in_tag(),
            },
            State::AttributeValueUnquoted => match c {
                Some(ch) if is_ws(ch) => self.state = State::BeforeAttributeName,
                Some('&') => {
                    let s = self.char_ref(true);
                    self.attr_value.push_str(&s);
                }
                Some('>') => self.emit_tag(),
                Some('\0') => {
                    self.error("unexpected-null-character");
                    self.attr_value.push('\u{FFFD}');
                }
                Some(ch) => {
                    if matches!(ch, '"' | '\'' | '<' | '=' | '`') {
                        self.error("unexpected-character-in-unquoted-attribute-value");
                    }
                    self.attr_value.push(ch);
                }
                None => self.eof_in_tag(),
            },
            State::AfterAttributeValueQuoted => match c {
                Some(ch) if is_ws(ch) => self.state = State::BeforeAttributeName,
                Some('/') => self.state = State::SelfClosingStartTag,
                Some('>') => self.emit_tag(),
                None => self.eof_in_tag(),
                Some(_) => {
                    self.error("missing-whitespace-between-attributes");
                    self.reconsume(c);
                    self.state = State::BeforeAttributeName;
                }
            },
            State::SelfClosingStartTag => match c {
                Some('>') => {
                    self.tag.self_closing = true;
                    self.emit_tag();
                }
                None => self.eof_in_tag(),
                Some(_) => {
                    self.error("unexpected-solidus-in-tag");
                    self.reconsume(c);
                    self.state = State::BeforeAttributeName;
                }
            },
            State::BogusComment => match c {
                Some('>') => {
                    self.emit_comment();
                    self.state = State::Text(TextMode::Data);
                }
                None => {
                    self.emit_comment();
                    self.emit_eof();
                }
                Some('\0') => self.comment.push('\u{FFFD}'),
                Some(ch) => self.comment.push(ch),
            },
            State::MarkupDeclarationOpen => {
                self.reconsume(c);
                if self.input[self.pos..].starts_with("--") {
                    self.pos += 2;
                    self.comment.clear();
                    self.state = State::CommentStart;
                } else if self.starts_with_ci("doctype") {
                    self.pos += 7;
                    self.state = State::Doctype;
                } else if self.input[self.pos..].starts_with("[CDATA[") {
                    // Outside foreign content CDATA is a bogus comment.
                    self.error("cdata-in-html-content");
                    self.comment.clear();
                    self.comment.push_str("[CDATA[");
                    self.pos += 7;
                    self.state = State::BogusComment;
                } else {
                    self.error("incorrectly-opened-comment");
                    self.comment.clear();
                    self.state = State::BogusComment;
                }
            }
            State::CommentStart => match c {
                Some('-') => self.state = State::CommentStartDash,
                Some('>') => {
                    self.error("abrupt-closing-of-empty-comment");
                    self.emit_comment();
                    self.state = State::Text(TextMode::Data);
                }
                _ => {
                    self.reconsume(c);
                    self.state = State::Comment;
                }
            },
            State::CommentStartDash => match c {
                Some('-') => self.state = State::CommentEnd,
                Some('>') => {
                    self.error("abrupt-closing-of-empty-comment");
                    self.emit_comment();
                    self.state = State::Text(TextMode::Data);
                }
                None => self.eof_in_comment(),
                Some(_) => {
                    self.comment.push('-');
                    self.reconsume(c);
                    self.state = State::Comment;
                }
            },
            State::Comment => match c {
                Some('<') => {
                    self.comment.push('<');
                    self.state = State::CommentLessThanSign;
                }
                Some('-') => self.state = State::CommentEndDash,
                Some('\0') => {
                    self.error("unexpected-null-character");
                    self.comment.push('\u{FFFD}');
                }
                Some(ch) => self.comment.push(ch),
                None => self.eof_in_comment(),
            },
            State::CommentLessThanSign => match c {
                Some('!') => {
                    self.comment.push('!');
                    self.state = State::CommentLessThanSignBang;
                }
                Some('<') => self.comment.push('<'),
                _ => {
                    self.reconsume(c);
                    self.state = State::Comment;
                }
            },
            State::CommentLessThanSignBang => {
                self.reconsume(c);
                self.state = if c == Some('-') {
                    self.pos += 1;
                    State::CommentLessThanSignBangDash
                } else {
                    State::Comment
                };
            }
            State::CommentLessThanSignBangDash => {
                self.reconsume(c);
                self.state = if c == Some('-') {
                    self.pos += 1;
                    State::CommentLessThanSignBangDashDash
                } else {
                    State::CommentEndDash
                };
            }
            State::CommentLessThanSignBangDashDash => {
                if !matches!(c, Some('>') | None) {
                    self.error("nested-comment");
                }
                self.reconsume(c);
                self.state = State::CommentEnd;
            }
            State::CommentEndDash => match c {
                Some('-') => self.state = State::CommentEnd,
                None => self.eof_in_comment(),
                Some(_) => {
                    self.comment.push('-');
                    self.reconsume(c);
                    self.state = State::Comment;
                }
            },
            State::CommentEnd => match c {
                Some('>') => {
                    self.emit_comment();
                    self.state = State::Text(TextMode::Data);
                }
                Some('!') => self.state = State::CommentEndBang,
                Some('-') => self.comment.push('-'),
                None => self.eof_in_comment(),
                Some(_) => {
                    self.comment.push_str("--");
                    self.reconsume(c);
                    self.state = State::Comment;
                }
            },
            State::CommentEndBang => match c {
                Some('-') => {
                    self.comment.push_str("--!");
                    self.state = State::CommentEndDash;
                }
                Some('>') => {
                    self.error("incorrectly-closed-comment");
                    self.emit_comment();
                    self.state = State::Text(TextMode::Data);
                }
                None => self.eof_in_comment(),
                Some(_) => {
                    self.comment.push_str("--!");
                    self.reconsume(c);
                    self.state = State::Comment;
                }
            },
            State::Doctype => match c {
                Some(ch) if is_ws(ch) => self.state = State::BeforeDoctypeName,
                Some('>') => {
                    self.reconsume(c);
                    self.state = State::BeforeDoctypeName;
                }
                None => self.eof_in_doctype(true),
                Some(_) => {
                    self.error("missing-whitespace-before-doctype-name");
                    self.reconsume(c);
                    self.state = State::BeforeDoctypeName;
                }
            },
            State::BeforeDoctypeName => match c {
                Some(ch) if is_ws(ch) => {}
                Some('>') => {
                    self.error("missing-doctype-name");
                    self.doctype = Doctype {
                        force_quirks: true,
                        ..Doctype::default()
                    };
                    self.emit_doctype();
                    self.state = State::Text(TextMode::Data);
                }
                None => self.eof_in_doctype(true),
                Some(ch) => {
                    self.doctype = Doctype::default();
                    let ch = if ch == '\0' {
                        '\u{FFFD}'
                    } else {
                        ch.to_ascii_lowercase()
                    };
                    self.doctype.name = Some(ch.to_string());
                    self.state = State::DoctypeName;
                }
            },
            State::DoctypeName => match c {
                Some(ch) if is_ws(ch) => self.state = State::AfterDoctypeName,
                Some('>') => {
                    self.emit_doctype();
                    self.state = State::Text(TextMode::Data);
                }
                None => self.eof_in_doctype(false),
                Some(ch) => {
                    let ch = if ch == '\0' {
                        '\u{FFFD}'
                    } else {
                        ch.to_ascii_lowercase()
                    };
                    self.doctype.name.get_or_insert_default().push(ch);
                }
            },
            State::AfterDoctypeName => match c {
                Some(ch) if is_ws(ch) => {}
                Some('>') => {
                    self.emit_doctype();
                    self.state = State::Text(TextMode::Data);
                }
                None => self.eof_in_doctype(false),
                Some(_) => {
                    self.reconsume(c);
                    if self.starts_with_ci("public") {
                        self.pos += 6;
                        self.state = State::AfterDoctypePublicKeyword;
                    } else if self.starts_with_ci("system") {
                        self.pos += 6;
                        self.state = State::AfterDoctypeSystemKeyword;
                    } else {
                        self.pos += c.map_or(0, char::len_utf8);
                        self.error("invalid-character-sequence-after-doctype-name");
                        self.doctype.force_quirks = true;
                        self.state = State::BogusDoctype;
                    }
                }
            },
            State::AfterDoctypePublicKeyword | State::BeforeDoctypePublicIdentifier => match c {
                Some(ch) if is_ws(ch) => self.state = State::BeforeDoctypePublicIdentifier,
                Some(q @ ('"' | '\'')) => {
                    self.doctype.public_id = Some(String::new());
                    self.state = State::DoctypePublicIdentifier(q);
                }
                _ => self.doctype_missing_identifier(c),
            },
            State::DoctypePublicIdentifier(quote) => match c {
                Some(ch) if ch == quote => self.state = State::AfterDoctypePublicIdentifier,
                Some('>') => self.abrupt_doctype_identifier(),
                None => self.eof_in_doctype(false),
                Some(ch) => {
                    let ch = if ch == '\0' { '\u{FFFD}' } else { ch };
                    self.doctype.public_id.get_or_insert_default().push(ch);
                }
            },
            State::AfterDoctypePublicIdentifier
            | State::BetweenDoctypePublicAndSystemIdentifiers => match c {
                Some(ch) if is_ws(ch) => {
                    self.state = State::BetweenDoctypePublicAndSystemIdentifiers;
                }
                Some('>') => {
                    self.emit_doctype();
                    self.state = State::Text(TextMode::Data);
                }
                Some(q @ ('"' | '\'')) => {
                    self.doctype.system_id = Some(String::new());
                    self.state = State::DoctypeSystemIdentifier(q);
                }
                None => self.eof_in_doctype(false),
                Some(_) => {
                    self.error("missing-quote-before-doctype-system-identifier");
                    self.doctype.force_quirks = true;
                    self.reconsume(c);
                    self.state = State::BogusDoctype;
                }
            },
            State::AfterDoctypeSystemKeyword | State::BeforeDoctypeSystemIdentifier => match c {
                Some(ch) if is_ws(ch) => self.state = State::BeforeDoctypeSystemIdentifier,
                Some(q @ ('"' | '\'')) => {
                    self.doctype.system_id = Some(String::new());
                    self.state = State::DoctypeSystemIdentifier(q);
                }
                _ => self.doctype_missing_identifier(c),
            },
            State::DoctypeSystemIdentifier(quote) => match c {
                Some(ch) if ch == quote => self.state = State::AfterDoctypeSystemIdentifier,
                Some('>') => self.abrupt_doctype_identifier(),
                None => self.eof_in_doctype(false),
                Some(ch) => {
                    let ch = if ch == '\0' { '\u{FFFD}' } else { ch };
                    self.doctype.system_id.get_or_insert_default().push(ch);
                }
            },
            State::AfterDoctypeSystemIdentifier => match c {
                Some(ch) if is_ws(ch) => {}
                Some('>') => {
                    self.emit_doctype();
                    self.state = State::Text(TextMode::Data);
                }
                None => self.eof_in_doctype(false),
                Some(_) => {
                    // Not a force-quirks condition.
                    self.error("unexpected-character-after-doctype-system-identifier");
                    self.reconsume(c);
                    self.state = State::BogusDoctype;
                }
            },
            State::BogusDoctype => match c {
                Some('>') => {
                    self.emit_doctype();
                    self.state = State::Text(TextMode::Data);
                }
                None => {
                    self.emit_doctype();
                    self.emit_eof();
                }
                Some(_) => {}
            },
        }
    }

    fn text_state(&mut self, mode: TextMode, c: Option<char>) {
        match (mode, c) {
            (_, None) => self.emit_eof(),
            (TextMode::Data, Some('<')) => self.state = State::TagOpen,
            (TextMode::Data | TextMode::Rcdata, Some('&')) => {
                let s = self.char_ref(false);
                self.text.push_str(&s);
            }
            (TextMode::Rcdata | TextMode::Rawtext | TextMode::ScriptData, Some('<')) => {
                self.state = State::RawLessThan(mode);
            }
            (TextMode::Data, Some('\0')) => {
                // Dropped here: every insertion mode we support ignores U+0000
                // in data (foreign content, which would keep it, is unsupported).
                self.error("unexpected-null-character");
            }
            (_, Some('\0')) => {
                self.error("unexpected-null-character");
                self.text.push('\u{FFFD}');
            }
            (_, Some(ch)) => self.text.push(ch),
        }
    }

    fn eof_in_tag(&mut self) {
        self.error("eof-in-tag");
        self.emit_eof();
    }

    fn eof_in_comment(&mut self) {
        self.error("eof-in-comment");
        self.emit_comment();
        self.emit_eof();
    }

    fn eof_in_doctype(&mut self, fresh: bool) {
        self.error("eof-in-doctype");
        if fresh {
            self.doctype = Doctype::default();
        }
        self.doctype.force_quirks = true;
        self.emit_doctype();
        self.emit_eof();
    }

    fn doctype_missing_identifier(&mut self, c: Option<char>) {
        match c {
            Some('>') => {
                self.error("missing-doctype-identifier");
                self.doctype.force_quirks = true;
                self.emit_doctype();
                self.state = State::Text(TextMode::Data);
            }
            None => self.eof_in_doctype(false),
            Some(_) => {
                self.error("missing-quote-before-doctype-identifier");
                self.doctype.force_quirks = true;
                self.reconsume(c);
                self.state = State::BogusDoctype;
            }
        }
    }

    fn abrupt_doctype_identifier(&mut self) {
        self.error("abrupt-doctype-identifier");
        self.doctype.force_quirks = true;
        self.emit_doctype();
        self.state = State::Text(TextMode::Data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(input: &str) -> Vec<Token> {
        let mut t = Tokenizer::new(input);
        let mut out = Vec::new();
        loop {
            let tok = t.next_token();
            if tok == Token::Eof {
                return out;
            }
            out.push(tok);
        }
    }

    fn start(name: &str, attrs: &[(&str, &str)]) -> Token {
        Token::StartTag(Tag {
            name: name.into(),
            attrs: attrs.iter().map(|(n, v)| Attribute::new(*n, *v)).collect(),
            self_closing: false,
        })
    }

    fn end(name: &str) -> Token {
        Token::EndTag(Tag {
            name: name.into(),
            ..Tag::default()
        })
    }

    fn chars(s: &str) -> Token {
        Token::Characters(s.into())
    }

    #[test]
    fn tags_text_and_case_folding() {
        assert_eq!(
            tokens("<P CLASS=a>Hi</P>"),
            [start("p", &[("class", "a")]), chars("Hi"), end("p")]
        );
    }

    #[test]
    fn attribute_forms() {
        assert_eq!(
            tokens(r#"<a href="x y" title='q' data-x=1 hidden>"#),
            [start(
                "a",
                &[
                    ("href", "x y"),
                    ("title", "q"),
                    ("data-x", "1"),
                    ("hidden", "")
                ]
            )]
        );
    }

    #[test]
    fn duplicate_attributes_keep_first() {
        let mut t = Tokenizer::new("<a x=1 x=2>");
        assert_eq!(t.next_token(), start("a", &[("x", "1")]));
        assert_eq!(t.errors()[0].code, "duplicate-attribute");
    }

    #[test]
    fn self_closing_flag() {
        let Token::StartTag(tag) = &tokens("<br/>")[0] else {
            panic!()
        };
        assert!(tag.self_closing);
    }

    #[test]
    fn comments() {
        assert_eq!(
            tokens("<!-- a -- b -->"),
            [Token::Comment(" a -- b ".into())]
        );
        assert_eq!(tokens("<!---->"), [Token::Comment(String::new())]);
        assert_eq!(tokens("<!-->"), [Token::Comment(String::new())]);
        assert_eq!(tokens("<!--x--!>"), [Token::Comment("x".into())]);
        assert_eq!(tokens("<?php x ?>"), [Token::Comment("?php x ?".into())]);
        assert_eq!(tokens("</ x>"), [Token::Comment(" x".into())]);
        assert_eq!(tokens("<!-- open"), [Token::Comment(" open".into())]);
    }

    #[test]
    fn doctypes() {
        assert_eq!(
            tokens("<!DOCTYPE html>"),
            [Token::Doctype(Doctype {
                name: Some("html".into()),
                ..Doctype::default()
            })]
        );
        assert_eq!(
            tokens(r#"<!doctype HTML PUBLIC "-//W3C//DTD HTML 4.01//EN" 'http://x'>"#),
            [Token::Doctype(Doctype {
                name: Some("html".into()),
                public_id: Some("-//W3C//DTD HTML 4.01//EN".into()),
                system_id: Some("http://x".into()),
                force_quirks: false,
            })]
        );
        let Token::Doctype(d) = &tokens("<!DOCTYPE html bogus>")[0] else {
            panic!()
        };
        assert!(d.force_quirks);
    }

    #[test]
    fn character_references() {
        assert_eq!(
            tokens("a&amp;b&lt;&#65;&#x42;&eacute"),
            [chars("a&b<AB\u{e9}")]
        );
        assert_eq!(
            tokens("&unknown; &; &#; &#x;"),
            [chars("&unknown; &; &#; &#x;")]
        );
        assert_eq!(
            tokens("&#0;&#x110000;&#xD800;"),
            [chars("\u{FFFD}\u{FFFD}\u{FFFD}")]
        );
        assert_eq!(tokens("&#128;&#x99;"), [chars("\u{20AC}\u{2122}")]);
        assert_eq!(tokens("&notit;"), [chars("\u{AC}it;")]);
    }

    #[test]
    fn character_references_in_attributes() {
        assert_eq!(
            tokens(r#"<a href="?a=1&copy=2&amp;c&copy;">"#),
            [start("a", &[("href", "?a=1&copy=2&c\u{a9}")])]
        );
    }

    #[test]
    fn rcdata_and_appropriate_end_tag() {
        let mut t = Tokenizer::new("<title>a<b>&amp;</tit</title>x");
        assert_eq!(t.next_token(), start("title", &[]));
        t.set_text_mode(TextMode::Rcdata);
        assert_eq!(t.next_token(), chars("a<b>&</tit"));
        assert_eq!(t.next_token(), end("title"));
        assert_eq!(t.next_token(), chars("x"));
    }

    #[test]
    fn rawtext_does_not_decode_references() {
        let mut t = Tokenizer::new("<style>a&amp;<p></style>");
        assert_eq!(t.next_token(), start("style", &[]));
        t.set_text_mode(TextMode::Rawtext);
        assert_eq!(t.next_token(), chars("a&amp;<p>"));
        assert_eq!(t.next_token(), end("style"));
    }

    #[test]
    fn plaintext_consumes_everything() {
        let mut t = Tokenizer::new("<plaintext></plaintext><b>");
        t.next_token();
        t.set_text_mode(TextMode::Plaintext);
        assert_eq!(t.next_token(), chars("</plaintext><b>"));
        assert_eq!(t.next_token(), Token::Eof);
    }

    #[test]
    fn newline_normalization_and_nulls() {
        assert_eq!(tokens("a\r\nb\rc\0d"), [chars("a\nb\ncd")]);
    }

    #[test]
    fn stray_less_than_is_text() {
        assert_eq!(tokens("a < b <"), [chars("a < b <")]);
    }

    #[test]
    fn eof_inside_tag_drops_tag() {
        assert_eq!(tokens("x<div class="), [chars("x")]);
    }
}
