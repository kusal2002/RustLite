//! Rule collection, indexing, cascade, and style computation for a document.

use std::collections::HashMap;
use std::rc::Rc;

use rustlite_css::{Combinator, Complex, SimpleSelector, Specificity, Stylesheet};
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
    /// Hashed id/class/tag keys that some ancestor of the subject must have
    /// (sorted). A rule whose keys are not all among an element's ancestors'
    /// keys cannot match it, which rejects most descendant selectors without
    /// walking the tree (the idea behind browsers' ancestor Bloom filters).
    ancestor_keys: Box<[u64]>,
    specificity: Specificity,
    origin: Origin,
    declarations: Rc<[PropertyDeclaration]>,
}

const KEY_ID: u8 = 0;
const KEY_CLASS: u8 = 1;
const KEY_TAG: u8 = 2;

/// FNV-1a over a kind byte and a name: cheap, and collisions only cost a
/// full selector match.
fn key_hash(kind: u8, name: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in std::iter::once(kind).chain(name.bytes()) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// All style rules that apply to a document, indexed by the rightmost
/// compound's most selective key so each element only tests likely rules.
#[derive(Debug)]
pub struct Stylist {
    rules: Vec<Rule>,
    by_id: HashMap<String, Vec<u32>>,
    by_class: HashMap<String, Vec<u32>>,
    by_tag: HashMap<String, Vec<u32>>,
    /// Rules whose subject's best key is an attribute, by attribute name.
    by_attr: HashMap<String, Vec<u32>>,
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
            by_attr: HashMap::new(),
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

    /// Keys required on ancestors of `selector`'s subject: those of every
    /// compound directly followed by a descendant or child combinator. Such a
    /// compound is an ancestor of the next one, which is the subject, one of
    /// its ancestors, or a sibling of either, so it is always an ancestor of
    /// the subject. A compound followed by a sibling combinator is not
    /// (in `.a + .b .c`, `.a` is a sibling of `.c`'s ancestor).
    fn ancestor_keys(&self, selector: &Complex) -> Box<[u64]> {
        let mut keys = Vec::new();
        for k in 0..selector.compounds.len().saturating_sub(1) {
            if !matches!(
                selector.combinators[k],
                Combinator::Descendant | Combinator::Child
            ) {
                continue;
            }
            for simple in &selector.compounds[k].simple {
                match simple {
                    SimpleSelector::Id(id) => keys.push(key_hash(KEY_ID, &self.key(id))),
                    SimpleSelector::Class(c) => keys.push(key_hash(KEY_CLASS, &self.key(c))),
                    SimpleSelector::Type(t) => {
                        keys.push(key_hash(KEY_TAG, &t.to_ascii_lowercase()));
                    }
                    _ => {}
                }
            }
        }
        keys.sort_unstable();
        keys.dedup();
        keys.into()
    }

    /// Sorted keys of every ancestor element of `el` (see [`Rule::ancestor_keys`]).
    fn ancestor_key_set(&self, doc: &Document, el: NodeId) -> Vec<u64> {
        let mut keys = Vec::new();
        for a in doc.ancestors(el) {
            let Some(tag) = doc.tag_name(a) else { continue };
            keys.push(key_hash(KEY_TAG, &tag.to_ascii_lowercase()));
            if let Some(id) = doc.attribute(a, "id") {
                keys.push(key_hash(KEY_ID, &self.key(id)));
            }
            if let Some(classes) = doc.attribute(a, "class") {
                for class in classes
                    .split(|c: char| c.is_ascii_whitespace())
                    .filter(|c| !c.is_empty())
                {
                    keys.push(key_hash(KEY_CLASS, &self.key(class)));
                }
            }
        }
        keys.sort_unstable();
        keys.dedup();
        keys
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
                // Attribute names are least selective (`[href]` matches many
                // elements) but still far better than testing every element.
                let bucket = bucket.or_else(|| {
                    subject.simple.iter().find_map(|s| match s {
                        SimpleSelector::Attribute(a) => Some((3, &a.name)),
                        _ => None,
                    })
                });
                match bucket {
                    Some((0, key)) => self.by_id.entry(self.key(key)).or_default().push(index),
                    Some((1, key)) => self.by_class.entry(self.key(key)).or_default().push(index),
                    Some((2, key)) => self.by_tag.entry(key.clone()).or_default().push(index),
                    Some((_, key)) => self.by_attr.entry(key.clone()).or_default().push(index),
                    None => self.universal.push(index),
                }
                self.rules.push(Rule {
                    ancestor_keys: self.ancestor_keys(selector),
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
        if !self.by_attr.is_empty()
            && let Some(element) = doc.element(el)
        {
            for attr in &element.attrs {
                // Selector names are lowercased; HTML parser names already are.
                if let Some(rules) = self.by_attr.get(&*attr.name) {
                    candidates.extend(rules);
                }
            }
        }
        candidates.sort_unstable();
        candidates.dedup();
        let ctx = MatchContext {
            doc,
            quirks: self.quirks,
        };
        let needs_ancestors = candidates
            .iter()
            .any(|&i| !self.rules[i as usize].ancestor_keys.is_empty());
        let ancestors = if needs_ancestors {
            self.ancestor_key_set(doc, el)
        } else {
            Vec::new()
        };
        candidates.retain(|&i| {
            let rule = &self.rules[i as usize];
            rule.ancestor_keys
                .iter()
                .all(|k| ancestors.binary_search(k).is_ok())
                && matches(ctx, &rule.selector, el)
        });
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

    /// Number of distinct style objects (shared styles count once).
    pub fn distinct_count(&self) -> usize {
        let mut seen: Vec<*const ComputedStyle> =
            self.styles.iter().flatten().map(Rc::as_ptr).collect();
        seen.sort_unstable();
        seen.dedup();
        seen.len()
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
    /// Some computed value used a viewport unit (`vw`, `vh`, `vmin`,
    /// `vmax`): styles must be recomputed when the viewport size changes.
    pub viewport_dependent: bool,
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
/// tree order. External sheets are not included (see [`stylist_with_external`]).
pub fn stylist_for(doc: &Document) -> Stylist {
    stylist_with_external(doc, |_| None)
}

/// Is `n` a `<link>` to a stylesheet that applies to all media?
pub fn is_stylesheet_link(doc: &Document, n: NodeId) -> bool {
    if !doc.is_element_named(n, "link") || doc.attribute(n, "href").is_none() {
        return false;
    }
    let rel = doc.attribute(n, "rel").unwrap_or("");
    let has = |token: &str| {
        rel.split_ascii_whitespace()
            .any(|t| t.eq_ignore_ascii_case(token))
    };
    let media_ok = doc.attribute(n, "media").is_none_or(|m| {
        let m = m.trim();
        m.is_empty() || m.eq_ignore_ascii_case("all") || m.eq_ignore_ascii_case("screen")
    });
    has("stylesheet") && !has("alternate") && media_ok
}

/// Builds the stylist for `doc` with `<style>` elements and loaded external
/// sheets (`external(link_node)`), in tree order as the cascade requires.
pub fn stylist_with_external<'a>(
    doc: &Document,
    external: impl Fn(NodeId) -> Option<&'a Stylesheet>,
) -> Stylist {
    let mut stylist = Stylist::new(doc.quirks_mode());
    for n in doc.descendants(doc.root()) {
        if is_css_style_element(doc, n) {
            let sheet = rustlite_css::parse_stylesheet(&doc.text_content(n));
            stylist.add_stylesheet(&sheet, Origin::Author);
        } else if is_stylesheet_link(doc, n)
            && let Some(sheet) = external(n)
        {
            stylist.add_stylesheet(sheet, Origin::Author);
        }
    }
    stylist
}

/// Cascade sort key: origin and importance, then inline-ness, specificity, order.
/// Presentational hints rank between user-agent and author normal declarations.
fn precedence(origin: Origin, important: bool, inline: bool) -> u8 {
    match (origin, important) {
        (Origin::UserAgent, false) => 0,
        (Origin::Author, false) => 2 + u8::from(inline),
        (Origin::Author, true) => 4 + u8::from(inline),
        (Origin::UserAgent, true) => 6,
    }
}

const HINT_PRECEDENCE: u8 = 1;

/// HTML "rules for parsing dimension values" (simplified): a non-negative
/// number with an optional `%`; trailing garbage after the number is ignored.
fn parse_dimension(value: &str) -> Option<crate::values::LengthPercentage> {
    use crate::values::{Length, LengthPercentage};
    let v = value.trim_start();
    let end = v
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(v.len());
    let number: f32 = v[..end]
        .parse()
        .ok()
        .filter(|n: &f32| n.is_finite() && *n >= 0.0)?;
    Some(if v[end..].starts_with('%') {
        LengthPercentage::Percentage(number)
    } else {
        LengthPercentage::Length(Length::px(number))
    })
}

/// Presentational hints (HTML §15.4.3 "Attributes for embedded content"):
/// `width`/`height` attributes of embedded content map to CSS properties.
fn presentational_hints(doc: &Document, el: NodeId) -> Vec<PropertyDeclaration> {
    use crate::properties::{Longhand, Value};
    use crate::values::LengthPercentageAuto;
    if !matches!(
        doc.tag_name(el),
        Some("img" | "canvas" | "video" | "iframe" | "embed" | "object")
    ) {
        return Vec::new();
    }
    [("width", Longhand::Width), ("height", Longhand::Height)]
        .into_iter()
        .filter_map(|(attr, property)| {
            let lp = parse_dimension(doc.attribute(el, attr)?)?;
            Some(PropertyDeclaration {
                property,
                value: Value::LengthPercentageAuto(LengthPercentageAuto::LengthPercentage(lp)),
                important: false,
            })
        })
        .collect()
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
    let viewport_used = std::cell::Cell::new(false);
    let mut ctx = ComputeContext {
        root_font_size: crate::computed::INITIAL_FONT_SIZE,
        viewport_width: viewport.0,
        viewport_height: viewport.1,
        viewport_used: &viewport_used,
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

        let hints = presentational_hints(doc, el);

        let share_key = (!is_root && inline.is_none() && hints.is_empty()).then(|| {
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
        for d in &hints {
            entries.push(((HINT_PRECEDENCE, Specificity::default(), 0), d));
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
    stats.viewport_dependent = viewport_used.get();
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
    fn ancestor_filter_keeps_every_true_match() {
        let html = "<!DOCTYPE html><style>            .box .x { color: rgb(1, 0, 0) }            #main > p span { color: rgb(2, 0, 0) }            .a + .b .c { color: rgb(3, 0, 0) }            .sib ~ .y { color: rgb(4, 0, 0) }            .missing .x { color: rgb(9, 9, 9) }            </style>            <div class='box other'><i class=x id=t1>1</i></div>            <div id=main><p><b><span id=t2>2</span></b></p></div>            <div class=a></div><div class=b><em class=c id=t3>3</em></div>            <p class=sib></p><q></q><p class=y id=t4>4</p>";
        assert_eq!(color_of(html, "t1"), "rgb(1, 0, 0)");
        assert_eq!(color_of(html, "t2"), "rgb(2, 0, 0)");
        assert_eq!(color_of(html, "t3"), "rgb(3, 0, 0)");
        assert_eq!(color_of(html, "t4"), "rgb(4, 0, 0)");
        // Quirks mode: ancestor class keys compare case-insensitively.
        assert_eq!(
            color_of(
                "<style>.BOX .x { color: rgb(5, 0, 0) }</style><div class=box><i class=x id=q>q</i></div>",
                "q"
            ),
            "rgb(5, 0, 0)"
        );
    }

    #[test]
    fn viewport_unit_use_is_reported() {
        let stats = |html: &str| {
            let out = rustlite_html::parse_document(html);
            let doc = &out.document;
            compute_styles(doc, &stylist_for(doc), (800.0, 600.0)).1
        };
        assert!(!stats("<!DOCTYPE html><p style='width: 50%'>x").viewport_dependent);
        assert!(stats("<!DOCTYPE html><p style='width: 50vw'>x").viewport_dependent);
        assert!(
            stats("<!DOCTYPE html><style>p { font-size: 2vmin }</style><p>x").viewport_dependent
        );
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

#[cfg(test)]
mod hint_tests {
    use super::*;

    fn width_of(html: &str) -> String {
        let out = rustlite_html::parse_document(html);
        let doc = &out.document;
        let (styles, _) = compute_styles(doc, &stylist_for(doc), (800.0, 600.0));
        let img = doc.find_element(doc.root(), "img").unwrap();
        crate::resolved_value(styles.get(img).unwrap(), "width").unwrap()
    }

    #[test]
    fn width_attribute_is_a_presentational_hint() {
        assert_eq!(width_of("<img width=120>"), "120px");
        assert_eq!(width_of("<img width='50%'>"), "50%");
        assert_eq!(
            width_of("<img width='80px'>"),
            "80px",
            "trailing garbage ignored"
        );
        assert_eq!(
            width_of("<img width=-5>"),
            "auto",
            "invalid values are ignored"
        );
        // Any author rule beats the hint, regardless of specificity.
        assert_eq!(
            width_of("<style>* { width: 10px }</style><img width=120>"),
            "10px"
        );
        // The UA sheet does not.
        assert_eq!(width_of("<img>"), "auto");
    }
}

#[cfg(test)]
mod external_tests {
    use super::*;

    #[test]
    fn external_sheets_cascade_in_document_order() {
        let out = rustlite_html::parse_document(
            "<style>p { color: rgb(1, 1, 1) }</style>\
             <link rel=stylesheet href=a.css>\
             <link rel='alternate stylesheet' href=alt.css>\
             <link rel=stylesheet media=print href=print.css>\
             <style>p.late { color: rgb(3, 3, 3) }</style><p id=a>x</p><p class=late id=b>y</p>",
        );
        let doc = &out.document;
        let sheet = rustlite_css::parse_stylesheet("p { color: rgb(2, 2, 2) }");
        let links: Vec<_> = doc
            .descendants(doc.root())
            .filter(|&n| doc.is_element_named(n, "link"))
            .collect();
        assert!(is_stylesheet_link(doc, links[0]));
        assert!(
            !is_stylesheet_link(doc, links[1]),
            "alternate sheets are off by default"
        );
        assert!(
            !is_stylesheet_link(doc, links[2]),
            "print-only sheets do not apply"
        );
        let stylist = stylist_with_external(doc, |n| (n == links[0]).then_some(&sheet));
        let (styles, _) = compute_styles(doc, &stylist, (800.0, 600.0));
        let color = |id: &str| {
            let n = doc
                .descendants(doc.root())
                .find(|&n| doc.attribute(n, "id") == Some(id))
                .unwrap();
            crate::resolved_value(styles.get(n).unwrap(), "color").unwrap()
        };
        // The linked sheet comes after the first <style>, before the second.
        assert_eq!(color("a"), "rgb(2, 2, 2)");
        assert_eq!(color("b"), "rgb(3, 3, 3)");
    }
}
