//! CSS tokenizer (CSS Syntax Module Level 3, §4).
//!
//! Comments are dropped. Input preprocessing (§3.3) is applied: CR, CRLF and
//! FF become LF, and U+0000 becomes U+FFFD.

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    Function(String),
    AtKeyword(String),
    /// `is_id` is true when the value would be a valid identifier.
    Hash {
        value: String,
        is_id: bool,
    },
    String(String),
    BadString,
    Url(String),
    BadUrl,
    Delim(char),
    Number {
        value: f64,
        is_integer: bool,
    },
    Percentage(f64),
    Dimension {
        value: f64,
        unit: String,
    },
    Whitespace,
    Cdo,
    Cdc,
    Colon,
    Semicolon,
    Comma,
    OpenSquare,
    CloseSquare,
    OpenParen,
    CloseParen,
    OpenCurly,
    CloseCurly,
}

fn preprocess(input: &str) -> Vec<char> {
    let mut out = Vec::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\x0C' => out.push('\n'),
            '\0' => out.push('\u{FFFD}'),
            c => out.push(c),
        }
    }
    out
}

fn is_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n')
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || !c.is_ascii()
}

fn is_ident_char(c: char) -> bool {
    is_ident_start(c) || c.is_ascii_digit() || c == '-'
}

fn is_non_printable(c: char) -> bool {
    matches!(c, '\0'..='\x08' | '\x0B' | '\x0E'..='\x1F' | '\x7F')
}

/// Tokenizes `input` completely. Never fails; malformed input produces
/// `BadString`/`BadUrl`/`Delim` tokens as the specification requires.
pub fn tokenize(input: &str) -> Vec<Token> {
    let mut t = Tokenizer {
        chars: preprocess(input),
        pos: 0,
    };
    let mut out = Vec::new();
    while let Some(tok) = t.next_token() {
        out.push(tok);
    }
    out
}

struct Tokenizer {
    chars: Vec<char>,
    pos: usize,
}

impl Tokenizer {
    fn peek(&self, n: usize) -> Option<char> {
        self.chars.get(self.pos + n).copied()
    }

    fn valid_escape_at(&self, n: usize) -> bool {
        self.peek(n) == Some('\\') && self.peek(n + 1) != Some('\n') && self.peek(n + 1).is_some()
    }

    fn starts_ident_at(&self, n: usize) -> bool {
        match self.peek(n) {
            Some('-') => {
                self.peek(n + 1)
                    .is_some_and(|c| is_ident_start(c) || c == '-')
                    || self.valid_escape_at(n + 1)
            }
            Some('\\') => self.valid_escape_at(n),
            Some(c) => is_ident_start(c),
            None => false,
        }
    }

    fn starts_number_at(&self, n: usize) -> bool {
        match self.peek(n) {
            Some('+' | '-') => {
                self.peek(n + 1).is_some_and(|c| c.is_ascii_digit())
                    || (self.peek(n + 1) == Some('.')
                        && self.peek(n + 2).is_some_and(|c| c.is_ascii_digit()))
            }
            Some('.') => self.peek(n + 1).is_some_and(|c| c.is_ascii_digit()),
            Some(c) => c.is_ascii_digit(),
            None => false,
        }
    }

    fn next_token(&mut self) -> Option<Token> {
        loop {
            // Comments.
            if self.peek(0) == Some('/') && self.peek(1) == Some('*') {
                self.pos += 2;
                while self.pos < self.chars.len()
                    && !(self.peek(0) == Some('*') && self.peek(1) == Some('/'))
                {
                    self.pos += 1;
                }
                self.pos = (self.pos + 2).min(self.chars.len());
                continue;
            }
            break;
        }
        let c = self.peek(0)?;
        self.pos += 1;
        let token = match c {
            c if is_ws(c) => {
                while self.peek(0).is_some_and(is_ws) {
                    self.pos += 1;
                }
                Token::Whitespace
            }
            '"' | '\'' => self.string(c),
            '#' => {
                if self.peek(0).is_some_and(is_ident_char) || self.valid_escape_at(0) {
                    let is_id = self.starts_ident_at(0);
                    Token::Hash {
                        value: self.ident_sequence(),
                        is_id,
                    }
                } else {
                    Token::Delim('#')
                }
            }
            '(' => Token::OpenParen,
            ')' => Token::CloseParen,
            '[' => Token::OpenSquare,
            ']' => Token::CloseSquare,
            '{' => Token::OpenCurly,
            '}' => Token::CloseCurly,
            ',' => Token::Comma,
            ':' => Token::Colon,
            ';' => Token::Semicolon,
            '+' | '.' => {
                self.pos -= 1;
                if self.starts_number_at(0) {
                    self.numeric()
                } else {
                    self.pos += 1;
                    Token::Delim(c)
                }
            }
            '-' => {
                self.pos -= 1;
                if self.starts_number_at(0) {
                    self.numeric()
                } else if self.peek(1) == Some('-') && self.peek(2) == Some('>') {
                    self.pos += 3;
                    Token::Cdc
                } else if self.starts_ident_at(0) {
                    self.ident_like()
                } else {
                    self.pos += 1;
                    Token::Delim('-')
                }
            }
            '<' => {
                if self.peek(0) == Some('!')
                    && self.peek(1) == Some('-')
                    && self.peek(2) == Some('-')
                {
                    self.pos += 3;
                    Token::Cdo
                } else {
                    Token::Delim('<')
                }
            }
            '@' => {
                if self.starts_ident_at(0) {
                    Token::AtKeyword(self.ident_sequence())
                } else {
                    Token::Delim('@')
                }
            }
            '\\' => {
                self.pos -= 1;
                if self.valid_escape_at(0) {
                    self.ident_like()
                } else {
                    self.pos += 1;
                    Token::Delim('\\')
                }
            }
            c if c.is_ascii_digit() => {
                self.pos -= 1;
                self.numeric()
            }
            c if is_ident_start(c) => {
                self.pos -= 1;
                self.ident_like()
            }
            c => Token::Delim(c),
        };
        Some(token)
    }

    /// Consumes an escape; the backslash has already been consumed.
    fn escape(&mut self) -> char {
        let Some(c) = self.peek(0) else {
            return '\u{FFFD}';
        };
        self.pos += 1;
        if !c.is_ascii_hexdigit() {
            return c;
        }
        let mut value = c.to_digit(16).unwrap_or(0);
        for _ in 0..5 {
            match self.peek(0).and_then(|c| c.to_digit(16)) {
                Some(d) => {
                    value = value * 16 + d;
                    self.pos += 1;
                }
                None => break,
            }
        }
        if self.peek(0).is_some_and(is_ws) {
            self.pos += 1;
        }
        match value {
            0 | 0xD800..=0xDFFF => '\u{FFFD}',
            v => char::from_u32(v).unwrap_or('\u{FFFD}'),
        }
    }

    fn ident_sequence(&mut self) -> String {
        let mut s = String::new();
        loop {
            match self.peek(0) {
                Some(c) if is_ident_char(c) => {
                    s.push(c);
                    self.pos += 1;
                }
                Some('\\') if self.valid_escape_at(0) => {
                    self.pos += 1;
                    s.push(self.escape());
                }
                _ => return s,
            }
        }
    }

    fn ident_like(&mut self) -> Token {
        let name = self.ident_sequence();
        if self.peek(0) != Some('(') {
            return Token::Ident(name);
        }
        self.pos += 1;
        if name.eq_ignore_ascii_case("url") {
            // Skip whitespace; if a quote follows, it is a url() function.
            let mut n = 0;
            while self.peek(n).is_some_and(is_ws) {
                n += 1;
            }
            if matches!(self.peek(n), Some('"' | '\'')) {
                return Token::Function(name);
            }
            self.pos += n;
            return self.url();
        }
        Token::Function(name)
    }

    fn url(&mut self) -> Token {
        let mut s = String::new();
        loop {
            let Some(c) = self.peek(0) else {
                return Token::Url(s);
            };
            self.pos += 1;
            match c {
                ')' => return Token::Url(s),
                c if is_ws(c) => {
                    while self.peek(0).is_some_and(is_ws) {
                        self.pos += 1;
                    }
                    match self.peek(0) {
                        Some(')') => {
                            self.pos += 1;
                            return Token::Url(s);
                        }
                        None => return Token::Url(s),
                        _ => return self.bad_url(),
                    }
                }
                '"' | '\'' | '(' => return self.bad_url(),
                c if is_non_printable(c) => return self.bad_url(),
                '\\' => {
                    if self.peek(0).is_some_and(|c| c != '\n') {
                        s.push(self.escape());
                    } else {
                        return self.bad_url();
                    }
                }
                c => s.push(c),
            }
        }
    }

    fn bad_url(&mut self) -> Token {
        while let Some(c) = self.peek(0) {
            self.pos += 1;
            if c == ')' {
                break;
            }
            if c == '\\' && self.peek(0).is_some() {
                self.escape();
            }
        }
        Token::BadUrl
    }

    fn string(&mut self, quote: char) -> Token {
        let mut s = String::new();
        loop {
            let Some(c) = self.peek(0) else {
                return Token::String(s);
            };
            self.pos += 1;
            match c {
                c if c == quote => return Token::String(s),
                '\n' => {
                    self.pos -= 1;
                    return Token::BadString;
                }
                '\\' => match self.peek(0) {
                    None => {}
                    Some('\n') => self.pos += 1,
                    Some(_) => s.push(self.escape()),
                },
                c => s.push(c),
            }
        }
    }

    fn number(&mut self) -> (f64, bool) {
        let start = self.pos;
        let mut is_integer = true;
        if matches!(self.peek(0), Some('+' | '-')) {
            self.pos += 1;
        }
        while self.peek(0).is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.peek(0) == Some('.') && self.peek(1).is_some_and(|c| c.is_ascii_digit()) {
            is_integer = false;
            self.pos += 1;
            while self.peek(0).is_some_and(|c| c.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(0), Some('e' | 'E')) {
            let sign = usize::from(matches!(self.peek(1), Some('+' | '-')));
            if self.peek(1 + sign).is_some_and(|c| c.is_ascii_digit()) {
                is_integer = false;
                self.pos += 1 + sign;
                while self.peek(0).is_some_and(|c| c.is_ascii_digit()) {
                    self.pos += 1;
                }
            }
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        // The scanned text is always a valid float literal; overflow gives ±inf,
        // which is clamped to the largest finite value.
        let value: f64 = text.parse().unwrap_or(0.0);
        (value.clamp(f64::MIN, f64::MAX), is_integer)
    }

    fn numeric(&mut self) -> Token {
        let (value, is_integer) = self.number();
        if self.starts_ident_at(0) {
            Token::Dimension {
                value,
                unit: self.ident_sequence(),
            }
        } else if self.peek(0) == Some('%') {
            self.pos += 1;
            Token::Percentage(value)
        } else {
            Token::Number { value, is_integer }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Token::*;

    fn ident(s: &str) -> Token {
        Ident(s.into())
    }

    #[test]
    fn basic_rule() {
        assert_eq!(
            tokenize("p{color:red}"),
            [
                ident("p"),
                OpenCurly,
                ident("color"),
                Colon,
                ident("red"),
                CloseCurly
            ]
        );
    }

    #[test]
    fn comments_and_whitespace() {
        assert_eq!(
            tokenize("a /* x */ b/**/c"),
            [ident("a"), Whitespace, Whitespace, ident("b"), ident("c")]
        );
    }

    #[test]
    fn numbers_and_dimensions() {
        assert_eq!(
            tokenize("10px -1.5em 50% +3 .5 1e2"),
            [
                Dimension {
                    value: 10.0,
                    unit: "px".into()
                },
                Whitespace,
                Dimension {
                    value: -1.5,
                    unit: "em".into()
                },
                Whitespace,
                Percentage(50.0),
                Whitespace,
                Number {
                    value: 3.0,
                    is_integer: true
                },
                Whitespace,
                Number {
                    value: 0.5,
                    is_integer: false
                },
                Whitespace,
                Number {
                    value: 100.0,
                    is_integer: false
                },
            ]
        );
    }

    #[test]
    fn hashes() {
        assert_eq!(
            tokenize("#main #123"),
            [
                Hash {
                    value: "main".into(),
                    is_id: true
                },
                Whitespace,
                Hash {
                    value: "123".into(),
                    is_id: false
                },
            ]
        );
    }

    #[test]
    fn strings_and_escapes() {
        assert_eq!(
            tokenize(r#""a\"b" 'c'"#),
            [String("a\"b".into()), Whitespace, String("c".into())]
        );
        assert_eq!(tokenize("\"open\nx"), [BadString, Whitespace, ident("x")]);
        assert_eq!(tokenize(r"\31 0"), [ident("10")]);
        assert_eq!(tokenize(r"a\:b"), [ident("a:b")]);
    }

    #[test]
    fn functions_and_urls() {
        assert_eq!(
            tokenize("rgb(1,2) url( a.png ) url('b')"),
            [
                Function("rgb".into()),
                Number {
                    value: 1.0,
                    is_integer: true
                },
                Comma,
                Number {
                    value: 2.0,
                    is_integer: true
                },
                CloseParen,
                Whitespace,
                Url("a.png".into()),
                Whitespace,
                Function("url".into()),
                String("b".into()),
                CloseParen,
            ]
        );
        assert_eq!(tokenize("url(a b)"), [BadUrl]);
    }

    #[test]
    fn at_keywords_cdo_cdc_and_delims() {
        assert_eq!(
            tokenize("@media <!-- --> > ~"),
            [
                AtKeyword("media".into()),
                Whitespace,
                Cdo,
                Whitespace,
                Cdc,
                Whitespace,
                Delim('>'),
                Whitespace,
                Delim('~')
            ]
        );
        assert_eq!(
            tokenize("--custom -x"),
            [ident("--custom"), Whitespace, ident("-x")]
        );
    }

    #[test]
    fn preprocessing() {
        assert_eq!(
            tokenize("a\r\nb\0"),
            [ident("a"), Whitespace, ident("b\u{FFFD}")]
        );
    }

    #[test]
    fn unterminated_comment_is_dropped() {
        assert_eq!(tokenize("a /* never closed"), [ident("a"), Whitespace]);
    }
}
