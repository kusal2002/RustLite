//! Rule and declaration parsing (CSS Syntax Module Level 3, §5).
//!
//! Style rules whose selectors fail to parse are dropped (as browsers do for
//! selectors they do not support). At-rules are currently skipped: `@media`,
//! `@import`, `@font-face`, `@supports` etc. are planned (docs/roadmap.md).

use crate::selectors::{SelectorList, parse_selector_list};
use crate::tokenizer::{Token, tokenize};

/// A preserved token, a function, or a simple block (§5.3).
#[derive(Debug, Clone, PartialEq)]
pub enum ComponentValue {
    Token(Token),
    Function {
        name: String,
        args: Vec<ComponentValue>,
    },
    /// `open` is `{`, `[` or `(`.
    Block {
        open: char,
        contents: Vec<ComponentValue>,
    },
}

impl ComponentValue {
    pub fn is_whitespace(&self) -> bool {
        matches!(self, ComponentValue::Token(Token::Whitespace))
    }
}

/// `name: value [!important]`. Names are ASCII-lowercased except custom
/// properties (`--*`), which are case-sensitive. The value has leading and
/// trailing whitespace removed.
#[derive(Debug, Clone, PartialEq)]
pub struct Declaration {
    pub name: String,
    pub value: Vec<ComponentValue>,
    pub important: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StyleRule {
    pub selectors: SelectorList,
    pub declarations: Vec<Declaration>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stylesheet {
    pub rules: Vec<StyleRule>,
    /// At-rules encountered and skipped (not yet supported).
    pub skipped_at_rules: usize,
    /// Style rules dropped because their selector list was invalid or unsupported.
    pub dropped_rules: usize,
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn next(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn component_value(&mut self, token: Token) -> ComponentValue {
        match token {
            Token::OpenCurly => self.block('{', Token::CloseCurly),
            Token::OpenSquare => self.block('[', Token::CloseSquare),
            Token::OpenParen => self.block('(', Token::CloseParen),
            Token::Function(name) => ComponentValue::Function {
                name,
                args: self.until_close(Token::CloseParen),
            },
            t => ComponentValue::Token(t),
        }
    }

    fn block(&mut self, open: char, close: Token) -> ComponentValue {
        ComponentValue::Block {
            open,
            contents: self.until_close(close),
        }
    }

    fn until_close(&mut self, close: Token) -> Vec<ComponentValue> {
        let mut out = Vec::new();
        while let Some(t) = self.next() {
            if t == close {
                break;
            }
            out.push(self.component_value(t));
        }
        out
    }

    /// Consumes an at-rule's prelude and optional block; the at-keyword has
    /// already been consumed.
    fn skip_at_rule(&mut self) {
        while let Some(t) = self.next() {
            match t {
                Token::Semicolon => return,
                Token::OpenCurly => {
                    self.block('{', Token::CloseCurly);
                    return;
                }
                t => {
                    self.component_value(t);
                }
            }
        }
    }

    fn stylesheet(&mut self) -> Stylesheet {
        let mut sheet = Stylesheet::default();
        while let Some(t) = self.next() {
            match t {
                Token::Whitespace | Token::Cdo | Token::Cdc => {}
                Token::AtKeyword(_) => {
                    self.skip_at_rule();
                    sheet.skipped_at_rules += 1;
                }
                t => {
                    // Qualified rule: prelude up to a `{}` block.
                    let mut prelude = vec![self.component_value(t)];
                    let mut block = None;
                    while let Some(t) = self.next() {
                        if t == Token::OpenCurly {
                            if let ComponentValue::Block { contents, .. } =
                                self.block('{', Token::CloseCurly)
                            {
                                block = Some(contents);
                            }
                            break;
                        }
                        prelude.push(self.component_value(t));
                    }
                    let Some(block) = block else {
                        // EOF before the block: the rule is invalid.
                        break;
                    };
                    match parse_selector_list(&prelude) {
                        Some(selectors) => sheet.rules.push(StyleRule {
                            selectors,
                            declarations: declarations_from(&block),
                        }),
                        None => sheet.dropped_rules += 1,
                    }
                }
            }
        }
        sheet
    }
}

fn to_component_values(input: &str) -> Vec<ComponentValue> {
    let mut p = Parser {
        tokens: tokenize(input),
        pos: 0,
    };
    let mut out = Vec::new();
    while let Some(t) = p.next() {
        out.push(p.component_value(t));
    }
    out
}

/// Parses a complete stylesheet. Never fails.
pub fn parse_stylesheet(input: &str) -> Stylesheet {
    let mut p = Parser {
        tokens: tokenize(input),
        pos: 0,
    };
    let sheet = p.stylesheet();
    debug_assert!(p.peek().is_none());
    sheet
}

/// Parses a declaration list such as the contents of a `style` attribute.
pub fn parse_declaration_list(input: &str) -> Vec<Declaration> {
    declarations_from(&to_component_values(input))
}

fn is_token(cv: &ComponentValue, t: &Token) -> bool {
    matches!(cv, ComponentValue::Token(x) if x == t)
}

/// §5.4.5 "consume a list of declarations" over already-parsed component values.
fn declarations_from(values: &[ComponentValue]) -> Vec<Declaration> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < values.len() {
        let cv = &values[i];
        // Everything up to the next top-level semicolon belongs to this item.
        let end = values[i..]
            .iter()
            .position(|v| is_token(v, &Token::Semicolon))
            .map_or(values.len(), |p| i + p);
        match cv {
            ComponentValue::Token(Token::Whitespace | Token::Semicolon) => {
                i += 1;
                continue;
            }
            ComponentValue::Token(Token::Ident(_)) => {
                if let Some(d) = declaration(&values[i..end]) {
                    out.push(d);
                }
            }
            // At-rules and anything else are errors here; skip to the semicolon.
            _ => {}
        }
        i = end + 1;
    }
    out
}

fn declaration(values: &[ComponentValue]) -> Option<Declaration> {
    let ComponentValue::Token(Token::Ident(name)) = &values[0] else {
        return None;
    };
    let mut rest = values[1..].iter().skip_while(|v| v.is_whitespace());
    if !rest.next().is_some_and(|v| is_token(v, &Token::Colon)) {
        return None;
    }
    let mut value: Vec<ComponentValue> = rest.cloned().collect();
    trim_whitespace(&mut value);

    // `! important` at the end (whitespace allowed between and around).
    let mut important = false;
    if let Some(ComponentValue::Token(Token::Ident(last))) = value.last()
        && last.eq_ignore_ascii_case("important")
    {
        let mut j = value.len() - 1;
        while j > 0 && value[j - 1].is_whitespace() {
            j -= 1;
        }
        if j > 0 && is_token(&value[j - 1], &Token::Delim('!')) {
            value.truncate(j - 1);
            trim_whitespace(&mut value);
            important = true;
        }
    }

    let name = if name.starts_with("--") {
        name.clone()
    } else {
        name.to_ascii_lowercase()
    };
    Some(Declaration {
        name,
        value,
        important,
    })
}

fn trim_whitespace(v: &mut Vec<ComponentValue>) {
    while v.last().is_some_and(ComponentValue::is_whitespace) {
        v.pop();
    }
    let lead = v.iter().take_while(|c| c.is_whitespace()).count();
    v.drain(..lead);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ident(s: &str) -> ComponentValue {
        ComponentValue::Token(Token::Ident(s.into()))
    }

    #[test]
    fn rules_and_declarations() {
        let sheet = parse_stylesheet("p { color : red ; margin:0 auto } div{}");
        assert_eq!(sheet.rules.len(), 2);
        let d = &sheet.rules[0].declarations;
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].name, "color");
        assert_eq!(d[0].value, [ident("red")]);
        assert_eq!(d[1].value.len(), 3);
        assert!(sheet.rules[1].declarations.is_empty());
    }

    #[test]
    fn important_flag() {
        let d = parse_declaration_list("color: red ! IMPORTANT; x: y!important;z:!important");
        assert!(d[0].important);
        assert_eq!(d[0].value, [ident("red")]);
        assert!(d[1].important);
        assert!(d[2].important);
        assert!(d[2].value.is_empty());
    }

    #[test]
    fn invalid_declarations_are_skipped() {
        let d = parse_declaration_list("color red; 12px: x; @foo bar; good: 1; COLOR: Blue");
        let names: Vec<_> = d.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["good", "color"]);
    }

    #[test]
    fn semicolons_inside_blocks_do_not_split() {
        let d = parse_declaration_list("a: f(x;y); b: 1");
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn at_rules_are_skipped_with_recovery() {
        let sheet = parse_stylesheet(
            "@import 'x.css'; @media screen { p { color: red } } h1 { color: blue }",
        );
        assert_eq!(sheet.skipped_at_rules, 2);
        assert_eq!(sheet.rules.len(), 1);
    }

    #[test]
    fn invalid_selector_drops_only_that_rule() {
        let sheet = parse_stylesheet("p:unknown-pseudo { a: b } p { c: d }");
        assert_eq!(sheet.dropped_rules, 1);
        assert_eq!(sheet.rules.len(), 1);
        assert_eq!(sheet.rules[0].declarations[0].name, "c");
    }

    #[test]
    fn unterminated_input_is_safe() {
        let sheet = parse_stylesheet("p { color: red");
        assert_eq!(sheet.rules.len(), 1);
        assert_eq!(parse_stylesheet("p ").rules.len(), 0);
    }

    #[test]
    fn custom_properties_keep_case() {
        let d = parse_declaration_list("--Main-Color: red");
        assert_eq!(d[0].name, "--Main-Color");
    }
}
