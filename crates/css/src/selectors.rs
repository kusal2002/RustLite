//! Selector parsing (Selectors Level 4 subset) and specificity.
//!
//! Supported: type, universal, `#id`, `.class`, attribute selectors (all seven
//! operators plus the `i`/`s` flags), combinators (descendant, `>`, `+`, `~`),
//! the pseudo-classes in [`PseudoClass`], and pseudo-elements (`::before`,
//! `::after`, ... and the legacy single-colon forms).
//!
//! Anything else (functional pseudo-classes such as `:not()`, namespaces,
//! unknown pseudo-classes) makes the whole selector list invalid, so the rule
//! is dropped, which is what a browser does for selectors it does not support.

use crate::parser::ComponentValue;
use crate::tokenizer::Token;

/// Specificity as (ids, classes/attributes/pseudo-classes, types/pseudo-elements).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub struct Specificity {
    pub a: u32,
    pub b: u32,
    pub c: u32,
}

impl From<(u32, u32, u32)> for Specificity {
    fn from((a, b, c): (u32, u32, u32)) -> Self {
        Self { a, b, c }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Combinator {
    /// Whitespace.
    Descendant,
    /// `>`
    Child,
    /// `+`
    NextSibling,
    /// `~`
    SubsequentSibling,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrOperator {
    /// `=`
    Equals,
    /// `~=` (whitespace-separated list contains)
    Includes,
    /// `|=` (equals or starts with value followed by `-`)
    DashMatch,
    /// `^=`
    Prefix,
    /// `$=`
    Suffix,
    /// `*=`
    Substring,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeSelector {
    /// ASCII-lowercased (HTML attribute names are case-insensitive).
    pub name: String,
    pub operation: Option<(AttrOperator, String)>,
    pub case_insensitive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PseudoClass {
    Root,
    FirstChild,
    LastChild,
    OnlyChild,
    FirstOfType,
    LastOfType,
    OnlyOfType,
    Empty,
    /// Unvisited link (`a`/`area` with `href`; RustLite has no history yet).
    Link,
    AnyLink,
    /// Never matches (privacy model: no visited state is exposed).
    Visited,
    /// User-action states. Valid but not matched until input state exists.
    Hover,
    Active,
    Focus,
    FocusVisible,
    FocusWithin,
}

impl PseudoClass {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "root" => Self::Root,
            "first-child" => Self::FirstChild,
            "last-child" => Self::LastChild,
            "only-child" => Self::OnlyChild,
            "first-of-type" => Self::FirstOfType,
            "last-of-type" => Self::LastOfType,
            "only-of-type" => Self::OnlyOfType,
            "empty" => Self::Empty,
            "link" => Self::Link,
            "any-link" => Self::AnyLink,
            "visited" => Self::Visited,
            "hover" => Self::Hover,
            "active" => Self::Active,
            "focus" => Self::Focus,
            "focus-visible" => Self::FocusVisible,
            "focus-within" => Self::FocusWithin,
            _ => return None,
        })
    }
}

const PSEUDO_ELEMENTS: &[&str] = &[
    "before",
    "after",
    "first-line",
    "first-letter",
    "marker",
    "placeholder",
    "selection",
    "backdrop",
    "file-selector-button",
];
const LEGACY_PSEUDO_ELEMENTS: &[&str] = &["before", "after", "first-line", "first-letter"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimpleSelector {
    /// ASCII-lowercased local name.
    Type(String),
    Universal,
    Id(String),
    Class(String),
    Attribute(AttributeSelector),
    PseudoClass(PseudoClass),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Compound {
    pub simple: Vec<SimpleSelector>,
    /// Set if the compound ends in a pseudo-element (only allowed on the last compound).
    pub pseudo_element: Option<String>,
}

/// A complex selector, stored left to right: `combinators[i]` sits between
/// `compounds[i]` and `compounds[i + 1]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Complex {
    pub compounds: Vec<Compound>,
    pub combinators: Vec<Combinator>,
}

impl Complex {
    pub fn specificity(&self) -> Specificity {
        let mut s = Specificity::default();
        for compound in &self.compounds {
            for simple in &compound.simple {
                match simple {
                    SimpleSelector::Id(_) => s.a += 1,
                    SimpleSelector::Class(_)
                    | SimpleSelector::Attribute(_)
                    | SimpleSelector::PseudoClass(_) => s.b += 1,
                    SimpleSelector::Type(_) => s.c += 1,
                    SimpleSelector::Universal => {}
                }
            }
            if compound.pseudo_element.is_some() {
                s.c += 1;
            }
        }
        s
    }

    /// The rightmost compound (the subject of the selector).
    pub fn subject(&self) -> &Compound {
        // Parsing guarantees at least one compound.
        &self.compounds[self.compounds.len() - 1]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorList(pub Vec<Complex>);

fn token(cv: &ComponentValue) -> Option<&Token> {
    match cv {
        ComponentValue::Token(t) => Some(t),
        _ => None,
    }
}

/// Parses a selector list from a rule prelude. `None` if any selector is invalid.
pub fn parse_selector_list(prelude: &[ComponentValue]) -> Option<SelectorList> {
    let mut list = Vec::new();
    for part in prelude.split(|cv| token(cv) == Some(&Token::Comma)) {
        list.push(parse_complex(part)?);
    }
    Some(SelectorList(list))
}

fn parse_complex(input: &[ComponentValue]) -> Option<Complex> {
    let mut compounds = Vec::new();
    let mut combinators = Vec::new();
    let mut current = Compound::default();
    let mut pending: Option<Combinator> = None;
    let mut saw_ws = false;
    let mut i = 0;

    let finish = |current: &mut Compound, compounds: &mut Vec<Compound>| -> bool {
        if current.simple.is_empty() && current.pseudo_element.is_none() {
            return false;
        }
        compounds.push(std::mem::take(current));
        true
    };

    while i < input.len() {
        let cv = &input[i];
        i += 1;
        if cv.is_whitespace() {
            saw_ws = true;
            continue;
        }
        let explicit = match token(cv) {
            Some(Token::Delim('>')) => Some(Combinator::Child),
            Some(Token::Delim('+')) => Some(Combinator::NextSibling),
            Some(Token::Delim('~')) => Some(Combinator::SubsequentSibling),
            _ => None,
        };
        if let Some(c) = explicit {
            if pending.is_some() || !finish(&mut current, &mut compounds) {
                return None;
            }
            pending = Some(c);
            saw_ws = false;
            continue;
        }
        // A simple selector. If a combinator (or whitespace) preceded it, the
        // previous compound is complete.
        if pending.is_some()
            || (saw_ws && !(current.simple.is_empty() && current.pseudo_element.is_none()))
        {
            if pending.is_none() && !finish(&mut current, &mut compounds) {
                return None;
            }
            combinators.push(pending.take().unwrap_or(Combinator::Descendant));
        }
        saw_ws = false;
        // Nothing may follow a pseudo-element in RustLite's supported subset.
        if current.pseudo_element.is_some() {
            return None;
        }
        match cv {
            ComponentValue::Token(Token::Ident(name)) if current.simple.is_empty() => {
                current
                    .simple
                    .push(SimpleSelector::Type(name.to_ascii_lowercase()));
            }
            ComponentValue::Token(Token::Delim('*')) if current.simple.is_empty() => {
                current.simple.push(SimpleSelector::Universal);
            }
            ComponentValue::Token(Token::Hash { value, is_id: true }) => {
                current.simple.push(SimpleSelector::Id(value.clone()));
            }
            ComponentValue::Token(Token::Delim('.')) => {
                let Some(ComponentValue::Token(Token::Ident(name))) = input.get(i) else {
                    return None;
                };
                i += 1;
                current.simple.push(SimpleSelector::Class(name.clone()));
            }
            ComponentValue::Block {
                open: '[',
                contents,
            } => {
                current
                    .simple
                    .push(SimpleSelector::Attribute(parse_attribute(contents)?));
            }
            ComponentValue::Token(Token::Colon) => {
                let double = input.get(i).and_then(token) == Some(&Token::Colon);
                if double {
                    i += 1;
                }
                let Some(ComponentValue::Token(Token::Ident(name))) = input.get(i) else {
                    return None;
                };
                i += 1;
                let lower = name.to_ascii_lowercase();
                if double || LEGACY_PSEUDO_ELEMENTS.contains(&lower.as_str()) {
                    if !PSEUDO_ELEMENTS.contains(&lower.as_str()) {
                        return None;
                    }
                    current.pseudo_element = Some(lower);
                } else {
                    current
                        .simple
                        .push(SimpleSelector::PseudoClass(PseudoClass::from_name(&lower)?));
                }
            }
            _ => return None,
        }
    }
    if pending.is_some() || !finish(&mut current, &mut compounds) {
        return None;
    }
    // Pseudo-elements are only valid on the subject compound.
    if compounds[..compounds.len() - 1]
        .iter()
        .any(|c| c.pseudo_element.is_some())
    {
        return None;
    }
    Some(Complex {
        compounds,
        combinators,
    })
}

fn parse_attribute(contents: &[ComponentValue]) -> Option<AttributeSelector> {
    let mut items = contents.iter().filter(|cv| !cv.is_whitespace()).peekable();
    let Some(ComponentValue::Token(Token::Ident(name))) = items.next() else {
        return None;
    };
    let name = name.to_ascii_lowercase();
    let Some(first) = items.next() else {
        return Some(AttributeSelector {
            name,
            operation: None,
            case_insensitive: false,
        });
    };
    let op = match token(first)? {
        Token::Delim('=') => AttrOperator::Equals,
        Token::Delim(c @ ('~' | '|' | '^' | '$' | '*')) => {
            if items.next().and_then(token) != Some(&Token::Delim('=')) {
                return None;
            }
            match c {
                '~' => AttrOperator::Includes,
                '|' => AttrOperator::DashMatch,
                '^' => AttrOperator::Prefix,
                '$' => AttrOperator::Suffix,
                _ => AttrOperator::Substring,
            }
        }
        _ => return None,
    };
    let value = match token(items.next()?)? {
        Token::Ident(v) | Token::String(v) => v.clone(),
        _ => return None,
    };
    let case_insensitive = match items.next() {
        None => false,
        Some(ComponentValue::Token(Token::Ident(flag))) if flag.eq_ignore_ascii_case("i") => true,
        Some(ComponentValue::Token(Token::Ident(flag))) if flag.eq_ignore_ascii_case("s") => false,
        Some(_) => return None,
    };
    if items.next().is_some() {
        return None;
    }
    Some(AttributeSelector {
        name,
        operation: Some((op, value)),
        case_insensitive,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_stylesheet;

    fn parse(sel: &str) -> Option<SelectorList> {
        let sheet = parse_stylesheet(&format!("{sel} {{}}"));
        sheet.rules.into_iter().next().map(|r| r.selectors)
    }

    fn spec(sel: &str) -> (u32, u32, u32) {
        let s = parse(sel).unwrap().0[0].specificity();
        (s.a, s.b, s.c)
    }

    #[test]
    fn specificity_examples() {
        assert_eq!(spec("*"), (0, 0, 0));
        assert_eq!(spec("li"), (0, 0, 1));
        assert_eq!(spec("ul li"), (0, 0, 2));
        assert_eq!(spec("ul ol+li"), (0, 0, 3));
        assert_eq!(spec("h1 + *[rel=up]"), (0, 1, 1));
        assert_eq!(spec("ul ol li.red"), (0, 1, 3));
        assert_eq!(spec("li.red.level"), (0, 2, 1));
        assert_eq!(spec("#x34y"), (1, 0, 0));
        assert_eq!(spec("a:hover::before"), (0, 1, 2));
    }

    #[test]
    fn structure() {
        let list = parse("DIV > p.a  span, #b").unwrap();
        assert_eq!(list.0.len(), 2);
        let c = &list.0[0];
        assert_eq!(c.combinators, [Combinator::Child, Combinator::Descendant]);
        assert_eq!(c.compounds[0].simple, [SimpleSelector::Type("div".into())]);
        assert_eq!(
            c.compounds[1].simple,
            [
                SimpleSelector::Type("p".into()),
                SimpleSelector::Class("a".into())
            ]
        );
    }

    #[test]
    fn combinators_without_spaces() {
        let c = &parse("a>b+c~d").unwrap().0[0];
        assert_eq!(
            c.combinators,
            [
                Combinator::Child,
                Combinator::NextSibling,
                Combinator::SubsequentSibling
            ]
        );
    }

    #[test]
    fn attribute_selectors() {
        let c = &parse(r#"[HREF] [lang|=en] [class~="a b"] [x^=y i]"#)
            .unwrap()
            .0[0];
        let attrs: Vec<_> = c
            .compounds
            .iter()
            .map(|c| match &c.simple[0] {
                SimpleSelector::Attribute(a) => a.clone(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(attrs[0].name, "href");
        assert_eq!(attrs[0].operation, None);
        assert_eq!(
            attrs[1].operation,
            Some((AttrOperator::DashMatch, "en".into()))
        );
        assert_eq!(
            attrs[2].operation,
            Some((AttrOperator::Includes, "a b".into()))
        );
        assert!(attrs[3].case_insensitive);
    }

    #[test]
    fn pseudo_elements() {
        let c = &parse("p:first-child::after").unwrap().0[0];
        assert_eq!(c.subject().pseudo_element.as_deref(), Some("after"));
        assert!(
            parse("p:before").unwrap().0[0]
                .subject()
                .pseudo_element
                .is_some()
        );
        assert!(
            parse("p::after span").is_none(),
            "pseudo-element must be last"
        );
        assert!(parse("p::bogus").is_none());
    }

    #[test]
    fn invalid_selectors() {
        for bad in [
            "> p", "p >", "p > > q", "p:not(a)", "p:nope", "a|b", "#1x", ".", "p,", "p..a", "[=x]",
            "[a=]", "[a=b c]", "div*",
        ] {
            assert!(parse(bad).is_none(), "{bad} should be invalid");
        }
    }
}
