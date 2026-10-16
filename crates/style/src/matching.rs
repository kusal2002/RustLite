//! Selector matching against the DOM (right to left).

use rustlite_css::{
    AttrOperator, AttributeSelector, Combinator, Complex, Compound, PseudoClass, SimpleSelector,
};
use rustlite_dom::{Document, NodeData, NodeId};

#[derive(Debug, Clone, Copy)]
pub(crate) struct MatchContext<'a> {
    pub doc: &'a Document,
    /// In quirks mode, class and ID selectors match ASCII case-insensitively.
    pub quirks: bool,
}

fn is_element(doc: &Document, n: NodeId) -> bool {
    matches!(doc.data(n), NodeData::Element(_))
}

fn parent_element(doc: &Document, n: NodeId) -> Option<NodeId> {
    doc.parent(n).filter(|&p| is_element(doc, p))
}

fn prev_element(doc: &Document, n: NodeId) -> Option<NodeId> {
    std::iter::successors(doc.prev_sibling(n), |&s| doc.prev_sibling(s))
        .find(|&s| is_element(doc, s))
}

fn next_element(doc: &Document, n: NodeId) -> Option<NodeId> {
    std::iter::successors(doc.next_sibling(n), |&s| doc.next_sibling(s))
        .find(|&s| is_element(doc, s))
}

fn eq(a: &str, b: &str, case_insensitive: bool) -> bool {
    if case_insensitive {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

fn has_class(value: &str, class: &str, ci: bool) -> bool {
    value
        .split(|c: char| c.is_ascii_whitespace())
        .any(|c| eq(c, class, ci))
}

fn matches_attribute(doc: &Document, el: NodeId, sel: &AttributeSelector) -> bool {
    let Some(actual) = doc.attribute(el, &sel.name) else {
        return false;
    };
    let Some((op, expected)) = &sel.operation else {
        return true;
    };
    let ci = sel.case_insensitive;
    let lower = |s: &str| {
        if ci {
            s.to_ascii_lowercase()
        } else {
            s.to_owned()
        }
    };
    let (actual_n, expected_n) = (lower(actual), lower(expected));
    match op {
        AttrOperator::Equals => actual_n == expected_n,
        AttrOperator::Includes => {
            !expected_n.is_empty()
                && !expected_n.contains(|c: char| c.is_ascii_whitespace())
                && has_class(&actual_n, &expected_n, false)
        }
        AttrOperator::DashMatch => {
            actual_n == expected_n
                || actual_n
                    .strip_prefix(&expected_n)
                    .is_some_and(|rest| rest.starts_with('-'))
        }
        AttrOperator::Prefix => !expected_n.is_empty() && actual_n.starts_with(&expected_n),
        AttrOperator::Suffix => !expected_n.is_empty() && actual_n.ends_with(&expected_n),
        AttrOperator::Substring => !expected_n.is_empty() && actual_n.contains(&expected_n),
    }
}

fn same_type_siblings<'d>(
    doc: &'d Document,
    el: NodeId,
    step: fn(&Document, NodeId) -> Option<NodeId>,
) -> impl Iterator<Item = NodeId> + 'd {
    let name = doc.tag_name(el).map(str::to_owned);
    std::iter::successors(step(doc, el), move |&s| step(doc, s))
        .filter(move |&s| doc.tag_name(s).map(str::to_owned) == name)
}

fn matches_pseudo_class(doc: &Document, el: NodeId, pc: PseudoClass) -> bool {
    match pc {
        PseudoClass::Root => doc.parent(el) == Some(doc.root()),
        PseudoClass::FirstChild => prev_element(doc, el).is_none(),
        PseudoClass::LastChild => next_element(doc, el).is_none(),
        PseudoClass::OnlyChild => {
            prev_element(doc, el).is_none() && next_element(doc, el).is_none()
        }
        PseudoClass::FirstOfType => same_type_siblings(doc, el, prev_element).next().is_none(),
        PseudoClass::LastOfType => same_type_siblings(doc, el, next_element).next().is_none(),
        PseudoClass::OnlyOfType => {
            same_type_siblings(doc, el, prev_element).next().is_none()
                && same_type_siblings(doc, el, next_element).next().is_none()
        }
        PseudoClass::Empty => doc.children(el).all(|c| match doc.data(c) {
            NodeData::Element(_) => false,
            NodeData::Text(t) => t.is_empty(),
            _ => true,
        }),
        PseudoClass::Link | PseudoClass::AnyLink => {
            matches!(doc.tag_name(el), Some("a" | "area")) && doc.attribute(el, "href").is_some()
        }
        PseudoClass::Visited
        | PseudoClass::Hover
        | PseudoClass::Active
        | PseudoClass::Focus
        | PseudoClass::FocusVisible
        | PseudoClass::FocusWithin => false,
    }
}

fn matches_compound(ctx: MatchContext<'_>, el: NodeId, compound: &Compound) -> bool {
    if compound.pseudo_element.is_some() {
        // Pseudo-elements are not generated yet, so they match no element.
        return false;
    }
    let doc = ctx.doc;
    compound.simple.iter().all(|s| match s {
        SimpleSelector::Type(name) => doc.tag_name(el) == Some(name.as_str()),
        SimpleSelector::Universal => true,
        SimpleSelector::Id(id) => doc
            .attribute(el, "id")
            .is_some_and(|v| eq(v, id, ctx.quirks)),
        SimpleSelector::Class(class) => doc
            .attribute(el, "class")
            .is_some_and(|v| has_class(v, class, ctx.quirks)),
        SimpleSelector::Attribute(a) => matches_attribute(doc, el, a),
        SimpleSelector::PseudoClass(pc) => matches_pseudo_class(doc, el, *pc),
    })
}

/// Does `el` match compounds `0..=k` of `sel`, with compound `k` matching `el`?
fn matches_from(ctx: MatchContext<'_>, sel: &Complex, k: usize, el: NodeId) -> bool {
    if !matches_compound(ctx, el, &sel.compounds[k]) {
        return false;
    }
    if k == 0 {
        return true;
    }
    let doc = ctx.doc;
    match sel.combinators[k - 1] {
        Combinator::Child => {
            parent_element(doc, el).is_some_and(|p| matches_from(ctx, sel, k - 1, p))
        }
        Combinator::Descendant => {
            std::iter::successors(parent_element(doc, el), |&p| parent_element(doc, p))
                .any(|p| matches_from(ctx, sel, k - 1, p))
        }
        Combinator::NextSibling => {
            prev_element(doc, el).is_some_and(|s| matches_from(ctx, sel, k - 1, s))
        }
        Combinator::SubsequentSibling => {
            std::iter::successors(prev_element(doc, el), |&s| prev_element(doc, s))
                .any(|s| matches_from(ctx, sel, k - 1, s))
        }
    }
}

pub(crate) fn matches(ctx: MatchContext<'_>, sel: &Complex, el: NodeId) -> bool {
    matches_from(ctx, sel, sel.compounds.len() - 1, el)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(html: &str, selector: &str, id: &str) -> bool {
        let out = rustlite_html::parse_document(html);
        let doc = &out.document;
        let el = doc
            .descendants(doc.root())
            .find(|&n| doc.attribute(n, "id") == Some(id))
            .expect("target id");
        let sheet = rustlite_css::parse_stylesheet(&format!("{selector} {{}}"));
        let sel = &sheet.rules.first().expect("valid selector").selectors.0[0];
        matches(MatchContext { doc, quirks: false }, sel, el)
    }

    const DOC: &str = r#"<div id=outer class="a b"><p id=first lang=en-US>x</p><span id=mid></span><p id=last data-x="foo bar"><b id=deep>y</b></p></div>"#;

    #[test]
    fn combinators() {
        assert!(check(DOC, "div b", "deep"));
        assert!(check(DOC, "div > p > b", "deep"));
        assert!(!check(DOC, "div > b", "deep"));
        assert!(check(DOC, "p + span", "mid"));
        assert!(check(DOC, "p ~ p", "last"));
        assert!(!check(DOC, "span + span", "mid"));
        assert!(check(DOC, ".a.b > #first", "first"));
    }

    #[test]
    fn attributes() {
        assert!(check(DOC, "[lang|=en]", "first"));
        assert!(check(DOC, "[data-x~=bar]", "last"));
        assert!(check(
            DOC,
            "[data-x^=fo][data-x$=ar][data-x*='o b']",
            "last"
        ));
        assert!(!check(DOC, "[data-x~='']", "last"));
        assert!(check(DOC, "[DATA-X='FOO BAR' i]", "last"));
        assert!(!check(DOC, "[data-x='FOO BAR']", "last"));
    }

    #[test]
    fn pseudo_classes() {
        assert!(check(DOC, "p:first-child", "first"));
        assert!(check(DOC, "p:last-child", "last"));
        assert!(check(DOC, "span:only-of-type", "mid"));
        assert!(check(DOC, "p:first-of-type", "first"));
        assert!(check(DOC, "span:empty", "mid"));
        assert!(!check(DOC, "p:empty", "first"));
        assert!(!check(DOC, "p:hover", "first"));
        assert!(!check(DOC, "p::before", "first"));
    }

    #[test]
    fn root_pseudo_class() {
        let out = rustlite_html::parse_document("<p>");
        let doc = &out.document;
        let html = doc.find_element(doc.root(), "html").unwrap();
        let sheet = rustlite_css::parse_stylesheet(":root {}");
        assert!(matches(
            MatchContext { doc, quirks: false },
            &sheet.rules[0].selectors.0[0],
            html
        ));
    }
}
