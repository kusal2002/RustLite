//! Rule collection, indexing, cascade, and style computation for a document.

use std::collections::HashMap;
use std::rc::Rc;

use rustlite_css::{Complex, SimpleSelector, Specificity, Stylesheet};
use rustlite_dom::{Document, NodeData, NodeId, QuirksMode};

use crate::computed::{ComputeContext, ComputedStyle, DeclaredValues, compute};
use crate::matching::{MatchContext, matches};
use crate::properties::{LONGHAND_COUNT, PropertyDeclaration, parse_declaration};

const UA_STYLESHEET: &str = include_str!("ua.css");

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    UserAgent,
    Author,
}

#[derive(Debug)]
struct Rule {
    selector: Complex,
    specificity: Specificity,
    origin: Origin,
    declarations: Rc<[PropertyDeclaration]>,
}

/// All style rules that apply to a document, indexed by the rightmost
/// compound's most selective key so each element only tests likely rules.
#[derive(Debug)]
pub struct Stylist {
    rules: Vec<Rule>,
    by_id: HashMap<String, Vec<u32>>,
    by_class: HashMap<String, Vec<u32>>,
    by_tag: HashMap<String, Vec<u32>>,
    universal: Vec<u32>,
    quirks: bool,
    stats: SheetStats,
}

/// Counters about the stylesheets fed to a [`Stylist`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SheetStats {
    pub author_sheets: usize,
    pub rules: usize,
    pub dropped_rules: usize,
    pub skipped_at_rules: usize,
}

impl Stylist {
    /// A stylist containing only the user-agent stylesheet.
    pub fn new(quirks_mode: QuirksMode) -> Self {
        let mut s = Self {
            rules: Vec::new(),
            by_id: HashMap::new(),
            by_class: HashMap::new(),
            by_tag: HashMap::new(),
            universal: Vec::new(),
            quirks: quirks_mode == QuirksMode::Quirks,
            stats: SheetStats::default(),
        };
        s.add_stylesheet(
            &rustlite_css::parse_stylesheet(UA_STYLESHEET),
            Origin::UserAgent,
        );
        s
    }

    pub fn stats(&self) -> SheetStats {
        self.stats
    }

    fn key(&self, s: &str) -> String {
        if self.quirks {
            s.to_ascii_lowercase()
        } else {
            s.to_owned()
        }
    }

    pub fn add_stylesheet(&mut self, sheet: &Stylesheet, origin: Origin) {
        if origin == Origin::Author {
            self.stats.author_sheets += 1;
            self.stats.dropped_rules += sheet.dropped_rules;
            self.stats.skipped_at_rules += sheet.skipped_at_rules;
        }
        for style_rule in &sheet.rules {
            let declarations: Rc<[PropertyDeclaration]> = style_rule
                .declarations
                .iter()
                .flat_map(parse_declaration)
                .collect();
            if declarations.is_empty() {
                continue;
            }
            for selector in &style_rule.selectors.0 {
                let index = u32::try_from(self.rules.len()).expect("fewer than 2^32 rules");
                let subject = selector.subject();
                let bucket = subject.simple.iter().find_map(|s| match s {
                    SimpleSelector::Id(id) => Some((0, id)),
                    _ => None,
                });
                let bucket = bucket.or_else(|| {
                    subject.simple.iter().find_map(|s| match s {
                        SimpleSelector::Class(c) => Some((1, c)),
                        _ => None,
                    })
                });
                let bucket = bucket.or_else(|| {
                    subject.simple.iter().find_map(|s| match s {
                        SimpleSelector::Type(t) => Some((2, t)),
                        _ => None,
                    })
                });
                match bucket {
                    Some((0, key)) => self.by_id.entry(self.key(key)).or_default().push(index),
                    Some((1, key)) => self.by_class.entry(self.key(key)).or_default().push(index),
                    Some((_, key)) => self.by_tag.entry(key.clone()).or_default().push(index),
                    None => self.universal.push(index),
                }
                self.rules.push(Rule {
                    selector: selector.clone(),
                    specificity: selector.specificity(),
                    origin,
                    declarations: declarations.clone(),
                });
                self.stats.rules += 1;
            }
        }
    }

    /// Indices of rules matching `el`, in source order.
    fn matching_rules(&self, doc: &Document, el: NodeId) -> Vec<u32> {
        let mut candidates: Vec<u32> = self.universal.clone();
        if let Some(id) = doc.attribute(el, "id")
            && let Some(rules) = self.by_id.get(&self.key(id))
        {
            candidates.extend(rules);
        }
        if let Some(classes) = doc.attribute(el, "class") {
            let mut seen: Vec<String> = Vec::new();
            for class in classes
                .split(|c: char| c.is_ascii_whitespace())
                .filter(|c| !c.is_empty())
            {
                let key = self.key(class);
                if seen.contains(&key) {
                    continue;
                }
                if let Some(rules) = self.by_class.get(&key) {
                    candidates.extend(rules);
                }
                seen.push(key);
            }
        }
        if let Some(rules) = doc.tag_name(el).and_then(|t| self.by_tag.get(t)) {
            candidates.extend(rules);
        }
        candidates.sort_unstable();
        candidates.dedup();
        let ctx = MatchContext {
            doc,
            quirks: self.quirks,
        };
        candidates.retain(|&i| matches(ctx, &self.rules[i as usize].selector, el));
        candidates
    }
}

/// Computed styles for every element of a document.
#[derive(Debug, Default)]
pub struct ComputedStyles {
    styles: Vec<Option<Rc<ComputedStyle>>>,
}

impl ComputedStyles {
    /// The computed style of an element (`None` for non-elements).
    pub fn get(&self, node: NodeId) -> Option<&ComputedStyle> {
        self.styles.get(node.index())?.as_deref()
    }

    /// Shared handle to an element's style, for structures that outlive a borrow.
    pub fn get_rc(&self, node: NodeId) -> Option<Rc<ComputedStyle>> {
        self.styles.get(node.index())?.clone()
    }
}

/// Counters from one style computation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StyleStats {
    pub sheets: SheetStats,
    pub elements: usize,
    /// Elements whose style was shared with an earlier element instead of computed.
    pub shared: usize,
    pub inline_styles: usize,
}

fn is_css_style_element(doc: &Document, n: NodeId) -> bool {
    if !doc.is_element_named(n, "style") {
        return false;
    }
    let type_ok = doc
        .attribute(n, "type")
        .is_none_or(|t| t.is_empty() || t.eq_ignore_ascii_case("text/css"));
    // Media queries are not supported yet: only sheets for all media apply.
    let media_ok = doc.attribute(n, "media").is_none_or(|m| {
        let m = m.trim();
        m.is_empty() || m.eq_ignore_ascii_case("all") || m.eq_ignore_ascii_case("screen")
    });
    type_ok && media_ok
}

/// Builds the stylist for `doc`: the UA sheet plus every `<style>` element in
/// tree order. (`<link rel=stylesheet>` needs networking, Phase 6.)
pub fn stylist_for(doc: &Document) -> Stylist {
    let mut stylist = Stylist::new(doc.quirks_mode());
    for n in doc.descendants(doc.root()) {
        if is_css_style_element(doc, n) {
            let sheet = rustlite_css::parse_stylesheet(&doc.text_content(n));
            stylist.add_stylesheet(&sheet, Origin::Author);
        }
    }
    stylist
}

/// Cascade sort key: origin and importance, then inline-ness, specificity, order.
fn precedence(origin: Origin, important: bool, inline: bool) -> u8 {
    match (origin, important) {
        (Origin::UserAgent, false) => 0,
        (Origin::Author, false) => 1 + u8::from(inline),
        (Origin::Author, true) => 3 + u8::from(inline),
        (Origin::UserAgent, true) => 5,
    }
}

/// Computes the style of every element in `doc`.
pub fn compute_styles(
    doc: &Document,
    stylist: &Stylist,
    viewport: (f32, f32),
) -> (ComputedStyles, StyleStats) {
    let mut styles: Vec<Option<Rc<ComputedStyle>>> = vec![None; doc.node_count()];
    let mut stats = StyleStats {
        sheets: stylist.stats(),
        ..StyleStats::default()
    };
    let mut ctx = ComputeContext {
        root_font_size: crate::computed::INITIAL_FONT_SIZE,
        viewport_width: viewport.0,
        viewport_height: viewport.1,
    };
    // Style sharing: same parent style + same matched rules + no inline style
    // ⇒ identical computed style.
    let mut share_cache: HashMap<(usize, Vec<u32>), Rc<ComputedStyle>> = HashMap::new();

    for el in doc.descendants(doc.root()) {
        if !matches!(doc.data(el), NodeData::Element(_)) {
            continue;
        }
        stats.elements += 1;
        let parent_style = doc.parent(el).and_then(|p| styles[p.index()].clone());
        let is_root = doc.parent(el) == Some(doc.root());
        let matched = stylist.matching_rules(doc, el);
        let inline = doc
            .attribute(el, "style")
            .map(|s| -> Vec<PropertyDeclaration> {
                rustlite_css::parse_declaration_list(s)
                    .iter()
                    .flat_map(parse_declaration)
                    .collect()
            });

        let share_key = (!is_root && inline.is_none()).then(|| {
            let parent_ptr = parent_style.as_ref().map_or(0, |p| Rc::as_ptr(p) as usize);
            (parent_ptr, matched.clone())
        });
        if let Some(key) = &share_key
            && let Some(shared) = share_cache.get(key)
        {
            styles[el.index()] = Some(shared.clone());
            stats.shared += 1;
            continue;
        }

        // Gather (key, declaration) pairs and apply in cascade order.
        let mut entries: Vec<((u8, Specificity, u32), &PropertyDeclaration)> = Vec::new();
        for &i in &matched {
            let rule = &stylist.rules[i as usize];
            for d in rule.declarations.iter() {
                entries.push((
                    (
                        precedence(rule.origin, d.important, false),
                        rule.specificity,
                        i,
                    ),
                    d,
                ));
            }
        }
        if let Some(inline) = &inline {
            stats.inline_styles += 1;
            for d in inline {
                entries.push((
                    (
                        precedence(Origin::Author, d.important, true),
                        Specificity::default(),
                        u32::MAX,
                    ),
                    d,
                ));
            }
        }
        entries.sort_by_key(|(k, _)| *k);
        let mut declared: DeclaredValues<'_> = [None; LONGHAND_COUNT];
        for (_, d) in &entries {
            declared[d.property as usize] = Some(&d.value);
        }

        let style = Rc::new(compute(&declared, parent_style.as_deref(), is_root, &ctx));
        if is_root {
            ctx.root_font_size = style.font_size;
        }
        if let Some(key) = share_key {
            share_cache.insert(key, style.clone());
        }
        styles[el.index()] = Some(style);
    }
    (ComputedStyles { styles }, stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn color_of(html: &str, id: &str) -> String {
        let out = rustlite_html::parse_document(html);
        let doc = &out.document;
        let stylist = stylist_for(doc);
        let (styles, _) = compute_styles(doc, &stylist, (800.0, 600.0));
        let el = doc
            .descendants(doc.root())
            .find(|&n| doc.attribute(n, "id") == Some(id))
            .unwrap();
        crate::resolved_value(styles.get(el).unwrap(), "color").unwrap()
    }

    // HTML §"Selectors": in quirks mode, class and ID selectors match ASCII
    // case-insensitively. (The style oracle cannot cover this: srcdoc
    // documents are never in quirks mode.)
    #[test]
    fn quirks_mode_class_and_id_matching() {
        let css = "<style>.Foo { color: rgb(1, 2, 3) } #BAR { color: rgb(4, 5, 6) }</style>";
        let quirks = format!("{css}<p class=foo id=a>a</p><p id=bar>b</p>");
        assert_eq!(color_of(&quirks, "a"), "rgb(1, 2, 3)");
        assert_eq!(color_of(&quirks, "bar"), "rgb(4, 5, 6)");
        let standards = format!("<!DOCTYPE html>{quirks}");
        assert_eq!(color_of(&standards, "a"), "rgb(0, 0, 0)");
        assert_eq!(color_of(&standards, "bar"), "rgb(0, 0, 0)");
    }

    #[test]
    fn identical_siblings_share_styles() {
        let html = format!("<!DOCTYPE html><ul>{}</ul>", "<li>x</li>".repeat(100));
        let out = rustlite_html::parse_document(&html);
        let doc = &out.document;
        let (styles, stats) = compute_styles(doc, &stylist_for(doc), (800.0, 600.0));
        assert!(stats.shared >= 99, "shared {}", stats.shared);
        let lis: Vec<_> = doc
            .descendants(doc.root())
            .filter(|&n| doc.is_element_named(n, "li"))
            .collect();
        assert!(std::ptr::eq(
            styles.get(lis[0]).unwrap(),
            styles.get(lis[99]).unwrap()
        ));
    }

    #[test]
    fn inline_styles_are_not_shared() {
        let out = rustlite_html::parse_document(
            "<!DOCTYPE html><p style='color:red'>a</p><p style='color:blue'>b</p>",
        );
        let doc = &out.document;
        let (styles, stats) = compute_styles(doc, &stylist_for(doc), (800.0, 600.0));
        assert_eq!(stats.inline_styles, 2);
        let ps: Vec<_> = doc
            .descendants(doc.root())
            .filter(|&n| doc.is_element_named(n, "p"))
            .collect();
        assert_ne!(
            styles.get(ps[0]).unwrap().color,
            styles.get(ps[1]).unwrap().color
        );
    }
}
