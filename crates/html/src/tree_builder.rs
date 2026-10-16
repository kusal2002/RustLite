//! HTML tree construction (WHATWG HTML §13.2.6).
//!
//! Implements every insertion mode except the template modes, including the
//! adoption agency algorithm, active-formatting reconstruction, foster
//! parenting, implied end tags, and quirks-mode detection.
//!
//! Known deviations (also listed in docs/compatibility.md):
//! - `<template>` is an ordinary element (no template contents fragment).
//! - SVG/MathML (foreign content) are parsed as ordinary HTML elements.
//! - Scripting is treated as **disabled** (`<noscript>` content is parsed), as
//!   RustLite does not execute scripts yet. This matches `DOMParser`.
//! - `<select>` follows the classic (pre-2025) content model.

use std::collections::HashMap;

use rustlite_dom::{Document, NodeData, NodeId, QuirksMode};

use crate::tokenizer::{Doctype, ParseError, Tag, TextMode, Token, Tokenizer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Initial,
    BeforeHtml,
    BeforeHead,
    InHead,
    InHeadNoscript,
    AfterHead,
    InBody,
    Text,
    InTable,
    InTableText,
    InCaption,
    InColumnGroup,
    InTableBody,
    InRow,
    InCell,
    InSelect,
    InSelectInTable,
    AfterBody,
    InFrameset,
    AfterFrameset,
    AfterAfterBody,
    AfterAfterFrameset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Default,
    ListItem,
    Button,
    Table,
    Select,
}

#[derive(Debug, Clone)]
enum Formatting {
    Marker,
    Element(NodeId, Tag),
}

/// Result of a mode handler: `Some(token)` means "reprocess in the current mode".
type Flow = Option<Token>;

const SPECIAL: &[&str] = &[
    "address",
    "applet",
    "area",
    "article",
    "aside",
    "base",
    "basefont",
    "bgsound",
    "blockquote",
    "body",
    "br",
    "button",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dir",
    "div",
    "dl",
    "dt",
    "embed",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hgroup",
    "hr",
    "html",
    "iframe",
    "img",
    "input",
    "keygen",
    "li",
    "link",
    "listing",
    "main",
    "marquee",
    "menu",
    "meta",
    "nav",
    "noembed",
    "noframes",
    "noscript",
    "object",
    "ol",
    "p",
    "param",
    "plaintext",
    "pre",
    "script",
    "search",
    "section",
    "select",
    "source",
    "style",
    "summary",
    "table",
    "tbody",
    "td",
    "template",
    "textarea",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "track",
    "ul",
    "wbr",
    "xmp",
];

const HEADINGS: &[&str] = &["h1", "h2", "h3", "h4", "h5", "h6"];

const FORMATTING: &[&str] = &[
    "a", "b", "big", "code", "em", "font", "i", "nobr", "s", "small", "strike", "strong", "tt", "u",
];

const IMPLIED_END: &[&str] = &[
    "dd", "dt", "li", "optgroup", "option", "p", "rb", "rp", "rt", "rtc",
];

const IMPLIED_END_THOROUGH: &[&str] = &[
    "caption", "colgroup", "dd", "dt", "li", "optgroup", "option", "p", "rb", "rp", "rt", "rtc",
    "tbody", "td", "tfoot", "th", "thead", "tr",
];

fn is_ws_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\x0C' | '\r' | ' ')
}

/// Splits leading HTML whitespace off `s`.
fn split_ws(s: &str) -> (&str, &str) {
    let end = s.find(|c| !is_ws_char(c)).unwrap_or(s.len());
    s.split_at(end)
}

fn is_all_ws(s: &str) -> bool {
    s.chars().all(is_ws_char)
}

fn tag(name: &str) -> Tag {
    Tag {
        name: name.into(),
        ..Tag::default()
    }
}

/// Output of [`parse_document`].
#[derive(Debug)]
pub struct ParseOutput {
    pub document: Document,
    /// Stored parse errors (tokenizer and tree construction), capped.
    pub errors: Vec<ParseError>,
    /// Total parse errors, including ones not stored.
    pub error_count: usize,
}

/// Parses a complete HTML document. Never fails: malformed input is repaired
/// the way the HTML standard prescribes.
pub fn parse_document(input: &str) -> ParseOutput {
    let mut builder = TreeBuilder::new(input);
    loop {
        let token = builder.tokenizer.next_token();
        let eof = token == Token::Eof;
        builder.process(token);
        if eof || builder.stopped {
            break;
        }
    }
    let mut errors = builder.tokenizer.errors().to_vec();
    errors.extend(builder.errors);
    errors.sort_by_key(|e| e.offset);
    ParseOutput {
        document: builder.doc,
        errors,
        error_count: builder.tokenizer.error_count() + builder.error_count,
    }
}

struct TreeBuilder {
    tokenizer: Tokenizer,
    doc: Document,
    mode: Mode,
    original_mode: Mode,
    open: Vec<NodeId>,
    /// Number of open elements per tag name, so scope checks for names that
    /// are not open at all return immediately instead of walking the stack.
    open_counts: HashMap<Box<str>, u32>,
    formatting: Vec<Formatting>,
    head: Option<NodeId>,
    form: Option<NodeId>,
    frameset_ok: bool,
    foster_parenting: bool,
    ignore_lf: bool,
    pending_table_text: String,
    stopped: bool,
    errors: Vec<ParseError>,
    error_count: usize,
}

impl TreeBuilder {
    fn new(input: &str) -> Self {
        Self {
            tokenizer: Tokenizer::new(input),
            doc: Document::new(),
            mode: Mode::Initial,
            original_mode: Mode::Initial,
            open: Vec::new(),
            open_counts: HashMap::new(),
            formatting: Vec::new(),
            head: None,
            form: None,
            frameset_ok: true,
            foster_parenting: false,
            ignore_lf: false,
            pending_table_text: String::new(),
            stopped: false,
            errors: Vec::new(),
            error_count: 0,
        }
    }

    fn error(&mut self, code: &'static str) {
        self.error_count += 1;
        if self.errors.len() < 256 {
            self.errors.push(ParseError { code, offset: 0 });
        }
    }

    // ----- dispatch ----------------------------------------------------------

    fn process(&mut self, mut token: Token) {
        if std::mem::take(&mut self.ignore_lf)
            && let Token::Characters(s) = &mut token
            && s.starts_with('\n')
        {
            s.remove(0);
            if s.is_empty() {
                return;
            }
        }
        let mut next = Some(token);
        while let Some(t) = next {
            if self.stopped {
                return;
            }
            next = self.step(t);
        }
    }

    fn step(&mut self, token: Token) -> Flow {
        match self.mode {
            Mode::Initial => self.initial(token),
            Mode::BeforeHtml => self.before_html(token),
            Mode::BeforeHead => self.before_head(token),
            Mode::InHead => self.in_head(token),
            Mode::InHeadNoscript => self.in_head_noscript(token),
            Mode::AfterHead => self.after_head(token),
            Mode::InBody => self.in_body(token),
            Mode::Text => self.text(token),
            Mode::InTable => self.in_table(token),
            Mode::InTableText => self.in_table_text(token),
            Mode::InCaption => self.in_caption(token),
            Mode::InColumnGroup => self.in_column_group(token),
            Mode::InTableBody => self.in_table_body(token),
            Mode::InRow => self.in_row(token),
            Mode::InCell => self.in_cell(token),
            Mode::InSelect => self.in_select(token),
            Mode::InSelectInTable => self.in_select_in_table(token),
            Mode::AfterBody => self.after_body(token),
            Mode::InFrameset => self.in_frameset(token),
            Mode::AfterFrameset => self.after_frameset(token),
            Mode::AfterAfterBody => self.after_after_body(token),
            Mode::AfterAfterFrameset => self.after_after_frameset(token),
        }
    }

    // ----- stack helpers -------------------------------------------------------

    fn count_open(&mut self, node: NodeId, added: bool) {
        let name = self.doc.tag_name(node).unwrap_or("");
        if let Some(c) = self.open_counts.get_mut(name) {
            if added {
                *c += 1;
            } else {
                *c = c.saturating_sub(1);
            }
        } else if added {
            self.open_counts.insert(name.into(), 1);
        }
    }

    fn is_open(&self, name: &str) -> bool {
        self.open_counts.get(name).is_some_and(|&c| c > 0)
    }

    fn push_open(&mut self, node: NodeId) {
        self.open.push(node);
        self.count_open(node, true);
    }

    fn truncate_open(&mut self, len: usize) {
        while self.open.len() > len {
            self.pop();
        }
    }

    fn remove_open_at(&mut self, i: usize) {
        let node = self.open.remove(i);
        self.count_open(node, false);
    }

    fn insert_open_at(&mut self, i: usize, node: NodeId) {
        self.open.insert(i, node);
        self.count_open(node, true);
    }

    fn replace_open_at(&mut self, i: usize, node: NodeId) {
        let old = std::mem::replace(&mut self.open[i], node);
        self.count_open(old, false);
        self.count_open(node, true);
    }

    fn current(&self) -> Option<NodeId> {
        self.open.last().copied()
    }

    fn name_of(&self, node: NodeId) -> &str {
        self.doc.tag_name(node).unwrap_or("")
    }

    fn current_is(&self, name: &str) -> bool {
        self.current().is_some_and(|n| self.name_of(n) == name)
    }

    fn current_is_one_of(&self, names: &[&str]) -> bool {
        self.current()
            .is_some_and(|n| names.contains(&self.name_of(n)))
    }

    fn is_special(&self, node: NodeId) -> bool {
        SPECIAL.contains(&self.name_of(node))
    }

    fn in_stack(&self, node: NodeId) -> bool {
        self.open.contains(&node)
    }

    fn in_scope_where(&self, scope: Scope, target: impl Fn(&str) -> bool) -> bool {
        for &node in self.open.iter().rev() {
            let name = self.name_of(node);
            if target(name) {
                return true;
            }
            let boundary = match scope {
                Scope::Select => !matches!(name, "optgroup" | "option"),
                Scope::Table => matches!(name, "html" | "table" | "template"),
                _ => {
                    matches!(
                        name,
                        "applet"
                            | "caption"
                            | "html"
                            | "table"
                            | "td"
                            | "th"
                            | "marquee"
                            | "object"
                            | "template"
                    ) || (scope == Scope::ListItem && matches!(name, "ol" | "ul"))
                        || (scope == Scope::Button && name == "button")
                }
            };
            if boundary {
                return false;
            }
        }
        false
    }

    fn in_scope(&self, name: &str, scope: Scope) -> bool {
        self.is_open(name) && self.in_scope_where(scope, |n| n == name)
    }

    fn in_scope_any(&self, names: &[&str], scope: Scope) -> bool {
        names.iter().any(|n| self.is_open(n)) && self.in_scope_where(scope, |n| names.contains(&n))
    }

    fn in_scope_node(&self, node: NodeId) -> bool {
        // Element-identity variant used by the adoption agency algorithm.
        for &n in self.open.iter().rev() {
            if n == node {
                return true;
            }
            if matches!(
                self.name_of(n),
                "applet"
                    | "caption"
                    | "html"
                    | "table"
                    | "td"
                    | "th"
                    | "marquee"
                    | "object"
                    | "template"
            ) {
                return false;
            }
        }
        false
    }

    fn pop(&mut self) -> Option<NodeId> {
        let node = self.open.pop()?;
        self.count_open(node, false);
        Some(node)
    }

    /// Pops elements until one named `name` has been popped.
    fn pop_until(&mut self, name: &str) {
        while let Some(n) = self.pop() {
            if self.name_of(n) == name {
                break;
            }
        }
    }

    fn pop_until_one_of(&mut self, names: &[&str]) {
        while let Some(n) = self.pop() {
            if names.contains(&self.name_of(n)) {
                break;
            }
        }
    }

    fn remove_from_stack(&mut self, node: NodeId) {
        if let Some(i) = self.open.iter().rposition(|&n| n == node) {
            self.remove_open_at(i);
        }
    }

    fn generate_implied_end_tags(&mut self, except: Option<&str>) {
        while let Some(cur) = self.current() {
            let name = self.name_of(cur);
            if IMPLIED_END.contains(&name) && Some(name) != except {
                self.pop();
            } else {
                break;
            }
        }
    }

    fn generate_all_implied_end_tags_thoroughly(&mut self) {
        while self.current_is_one_of(IMPLIED_END_THOROUGH) {
            self.pop();
        }
    }

    fn close_p_if_in_button_scope(&mut self) {
        if self.in_scope("p", Scope::Button) {
            self.close_p();
        }
    }

    fn close_p(&mut self) {
        self.generate_implied_end_tags(Some("p"));
        if !self.current_is("p") {
            self.error("unexpected-open-element-when-closing-p");
        }
        self.pop_until("p");
    }

    fn clear_to_context(&mut self, names: &[&str]) {
        while !self.current_is_one_of(names) && self.open.len() > 1 {
            self.pop();
        }
    }

    fn clear_to_table_context(&mut self) {
        self.clear_to_context(&["table", "template", "html"]);
    }

    fn clear_to_table_body_context(&mut self) {
        self.clear_to_context(&["tbody", "tfoot", "thead", "template", "html"]);
    }

    fn clear_to_row_context(&mut self) {
        self.clear_to_context(&["tr", "template", "html"]);
    }

    fn reset_insertion_mode(&mut self) {
        for (i, &node) in self.open.iter().enumerate().rev() {
            let last = i == 0;
            let mode = match self.name_of(node) {
                "select" => {
                    let in_table = self.open[..i]
                        .iter()
                        .rev()
                        .take_while(|&&a| self.name_of(a) != "template")
                        .any(|&a| self.name_of(a) == "table");
                    Some(if in_table {
                        Mode::InSelectInTable
                    } else {
                        Mode::InSelect
                    })
                }
                "td" | "th" if !last => Some(Mode::InCell),
                "tr" => Some(Mode::InRow),
                "tbody" | "thead" | "tfoot" => Some(Mode::InTableBody),
                "caption" => Some(Mode::InCaption),
                "colgroup" => Some(Mode::InColumnGroup),
                "table" => Some(Mode::InTable),
                "head" if !last => Some(Mode::InHead),
                "body" => Some(Mode::InBody),
                "frameset" => Some(Mode::InFrameset),
                "html" => Some(if self.head.is_none() {
                    Mode::BeforeHead
                } else {
                    Mode::AfterHead
                }),
                _ if last => Some(Mode::InBody),
                _ => None,
            };
            if let Some(mode) = mode {
                self.mode = mode;
                return;
            }
        }
        self.mode = Mode::InBody;
    }

    // ----- insertion -----------------------------------------------------------

    /// The appropriate place for inserting a node (§13.2.6.1), as (parent, before).
    fn insertion_location(&self, override_target: Option<NodeId>) -> (NodeId, Option<NodeId>) {
        let target = override_target
            .or_else(|| self.current())
            .unwrap_or(self.doc.root());
        if self.foster_parenting
            && matches!(
                self.name_of(target),
                "table" | "tbody" | "tfoot" | "thead" | "tr"
            )
        {
            if let Some(ti) = self.open.iter().rposition(|&n| self.name_of(n) == "table") {
                let table = self.open[ti];
                if let Some(parent) = self.doc.parent(table) {
                    return (parent, Some(table));
                }
                if ti > 0 {
                    return (self.open[ti - 1], None);
                }
            }
            return (self.open.first().copied().unwrap_or(self.doc.root()), None);
        }
        (target, None)
    }

    fn insert_node_at(&mut self, node: NodeId, location: (NodeId, Option<NodeId>)) {
        let (parent, before) = location;
        // Locations come from the open-element stack, so they are valid.
        if self.doc.insert_before(parent, node, before).is_err() {
            self.error("internal-insertion-failure");
        }
    }

    fn create_element(&mut self, tag: &Tag) -> NodeId {
        self.doc
            .create_element(tag.name.as_str(), tag.attrs.clone())
    }

    fn insert_element(&mut self, tag: &Tag) -> NodeId {
        let location = self.insertion_location(None);
        let node = self.create_element(tag);
        self.insert_node_at(node, location);
        self.push_open(node);
        node
    }

    /// Inserts a void element (pushed and immediately popped).
    fn insert_void(&mut self, tag: &Tag) {
        self.insert_element(tag);
        self.pop();
    }

    fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let (parent, before) = self.insertion_location(None);
        if matches!(self.doc.data(parent), NodeData::Document) {
            return;
        }
        if self.doc.insert_text(parent, text, before).is_err() {
            self.error("internal-insertion-failure");
        }
    }

    fn insert_comment(&mut self, text: String, parent: Option<NodeId>) {
        let location = match parent {
            Some(p) => (p, None),
            None => self.insertion_location(None),
        };
        let node = self.doc.create_comment(text);
        self.insert_node_at(node, location);
    }

    fn generic_text_element(&mut self, tag: &Tag, mode: TextMode) {
        self.insert_element(tag);
        self.tokenizer.set_text_mode(mode);
        self.original_mode = self.mode;
        self.mode = Mode::Text;
    }

    // ----- active formatting elements ------------------------------------------

    fn push_formatting(&mut self, node: NodeId, tag: &Tag) {
        // Noah's Ark clause: at most three identical entries after the last marker.
        let mut same = Vec::new();
        for (i, entry) in self.formatting.iter().enumerate().rev() {
            match entry {
                Formatting::Marker => break,
                Formatting::Element(_, t) => {
                    if t.name == tag.name && same_attributes(t, tag) {
                        same.push(i);
                    }
                }
            }
        }
        if same.len() >= 3 {
            // `same` is in reverse order; the last entry is the earliest.
            self.formatting.remove(same[same.len() - 1]);
        }
        self.formatting.push(Formatting::Element(node, tag.clone()));
    }

    fn formatting_index(&self, node: NodeId) -> Option<usize> {
        self.formatting
            .iter()
            .position(|e| matches!(e, Formatting::Element(n, _) if *n == node))
    }

    fn clear_formatting_to_marker(&mut self) {
        while let Some(e) = self.formatting.pop() {
            if matches!(e, Formatting::Marker) {
                break;
            }
        }
    }

    fn reconstruct_formatting(&mut self) {
        let Some(last) = self.formatting.last() else {
            return;
        };
        match last {
            Formatting::Marker => return,
            Formatting::Element(n, _) if self.in_stack(*n) => return,
            Formatting::Element(..) => {}
        }
        let mut i = self.formatting.len() - 1;
        while i > 0 {
            match &self.formatting[i - 1] {
                Formatting::Marker => break,
                Formatting::Element(n, _) if self.in_stack(*n) => break,
                Formatting::Element(..) => i -= 1,
            }
        }
        for j in i..self.formatting.len() {
            let Formatting::Element(_, tag) = &self.formatting[j] else {
                continue;
            };
            let tag = tag.clone();
            let node = self.insert_element(&tag);
            self.formatting[j] = Formatting::Element(node, tag);
        }
    }

    /// The adoption agency algorithm. Returns `true` if the caller should
    /// fall back to "any other end tag" handling.
    fn adoption_agency(&mut self, subject: &str) -> bool {
        if let Some(cur) = self.current()
            && self.name_of(cur) == subject
            && self.formatting_index(cur).is_none()
        {
            self.pop();
            return false;
        }
        for _ in 0..8 {
            // Step 4.3: last formatting element with this name after the last marker.
            let fe_entry = self
                .formatting
                .iter()
                .enumerate()
                .rev()
                .take_while(|(_, e)| !matches!(e, Formatting::Marker))
                .find_map(|(i, e)| match e {
                    Formatting::Element(n, t) if t.name == subject => Some((i, *n, t.clone())),
                    _ => None,
                });
            let Some((fe_index, fe, fe_tag)) = fe_entry else {
                return true;
            };
            let Some(fe_stack) = self.open.iter().position(|&n| n == fe) else {
                self.error("adoption-agency-1.2");
                self.formatting.remove(fe_index);
                return false;
            };
            if !self.in_scope_node(fe) {
                self.error("adoption-agency-4.4");
                return false;
            }
            if self.current() != Some(fe) {
                self.error("adoption-agency-1.3");
            }
            let furthest = self.open[fe_stack + 1..]
                .iter()
                .position(|&n| self.is_special(n))
                .map(|i| fe_stack + 1 + i);
            let Some(fb_stack) = furthest else {
                self.truncate_open(fe_stack);
                self.formatting.remove(fe_index);
                return false;
            };
            let furthest_block = self.open[fb_stack];
            let common_ancestor = self.open[fe_stack - 1];

            // Bookmark: the new element replaces `fe`, or goes after `insert_after`.
            let mut bookmark_after: Option<NodeId> = None;
            let mut node_index = fb_stack;
            let mut last_node = furthest_block;
            let mut counter = 0;
            loop {
                counter += 1;
                node_index -= 1;
                let node = self.open[node_index];
                if node == fe {
                    break;
                }
                if counter > 3
                    && let Some(i) = self.formatting_index(node)
                {
                    self.formatting.remove(i);
                }
                let Some(afe_i) = self.formatting_index(node) else {
                    self.remove_open_at(node_index);
                    continue;
                };
                let Formatting::Element(_, node_tag) = &self.formatting[afe_i] else {
                    unreachable!("formatting_index only returns element entries")
                };
                let node_tag = node_tag.clone();
                let new_node = self.create_element(&node_tag);
                self.formatting[afe_i] = Formatting::Element(new_node, node_tag);
                self.replace_open_at(node_index, new_node);
                if last_node == furthest_block {
                    bookmark_after = Some(new_node);
                }
                if self.doc.append_child(new_node, last_node).is_err() {
                    self.error("internal-insertion-failure");
                }
                last_node = new_node;
            }

            self.doc.detach(last_node);
            let location = self.insertion_location(Some(common_ancestor));
            self.insert_node_at(last_node, location);

            let new_element = self.create_element(&fe_tag);
            if self
                .doc
                .reparent_children(furthest_block, new_element)
                .is_err()
                || self.doc.append_child(furthest_block, new_element).is_err()
            {
                self.error("internal-insertion-failure");
            }

            let entry = Formatting::Element(new_element, fe_tag);
            match bookmark_after.and_then(|n| self.formatting_index(n)) {
                Some(i) => {
                    self.formatting.insert(i + 1, entry);
                    if let Some(old) = self.formatting_index(fe) {
                        self.formatting.remove(old);
                    }
                }
                None => {
                    if let Some(old) = self.formatting_index(fe) {
                        self.formatting[old] = entry;
                    }
                }
            }

            self.remove_from_stack(fe);
            if let Some(fb) = self.open.iter().position(|&n| n == furthest_block) {
                self.insert_open_at(fb + 1, new_element);
            }
        }
        false
    }

    // ----- insertion modes -------------------------------------------------------

    fn initial(&mut self, token: Token) -> Flow {
        match token {
            Token::Characters(s) => {
                let (_, rest) = split_ws(&s);
                if rest.is_empty() {
                    return None;
                }
                self.initial_anything_else(Token::Characters(rest.to_owned()))
            }
            Token::Comment(c) => {
                let root = self.doc.root();
                self.insert_comment(c, Some(root));
                None
            }
            Token::Doctype(d) => {
                let mode = quirks_mode_for(&d);
                if d.name.as_deref() != Some("html")
                    || d.public_id.is_some()
                    || d.system_id
                        .as_deref()
                        .is_some_and(|s| s != "about:legacy-compat")
                {
                    self.error("non-conforming-doctype");
                }
                let node = self.doc.create_doctype(
                    d.name.unwrap_or_default(),
                    d.public_id.unwrap_or_default(),
                    d.system_id.unwrap_or_default(),
                );
                let root = self.doc.root();
                self.insert_node_at(node, (root, None));
                self.doc.set_quirks_mode(mode);
                self.mode = Mode::BeforeHtml;
                None
            }
            other => self.initial_anything_else(other),
        }
    }

    fn initial_anything_else(&mut self, token: Token) -> Flow {
        self.error("missing-doctype");
        self.doc.set_quirks_mode(QuirksMode::Quirks);
        self.mode = Mode::BeforeHtml;
        Some(token)
    }

    fn before_html(&mut self, token: Token) -> Flow {
        match token {
            Token::Doctype(_) => {
                self.error("unexpected-doctype");
                None
            }
            Token::Comment(c) => {
                let root = self.doc.root();
                self.insert_comment(c, Some(root));
                None
            }
            Token::Characters(s) => {
                let (_, rest) = split_ws(&s);
                if rest.is_empty() {
                    return None;
                }
                self.before_html_anything_else(Token::Characters(rest.to_owned()))
            }
            Token::StartTag(t) if t.name == "html" => {
                let node = self.create_element(&t);
                let root = self.doc.root();
                self.insert_node_at(node, (root, None));
                self.push_open(node);
                self.mode = Mode::BeforeHead;
                None
            }
            Token::EndTag(t) if !matches!(t.name.as_str(), "head" | "body" | "html" | "br") => {
                self.error("unexpected-end-tag");
                None
            }
            other => self.before_html_anything_else(other),
        }
    }

    fn before_html_anything_else(&mut self, token: Token) -> Flow {
        let node = self.doc.create_element("html", Vec::new());
        let root = self.doc.root();
        self.insert_node_at(node, (root, None));
        self.push_open(node);
        self.mode = Mode::BeforeHead;
        Some(token)
    }

    fn before_head(&mut self, token: Token) -> Flow {
        match token {
            Token::Characters(s) => {
                let (_, rest) = split_ws(&s);
                if rest.is_empty() {
                    return None;
                }
                self.before_head_anything_else(Token::Characters(rest.to_owned()))
            }
            Token::Comment(c) => {
                self.insert_comment(c, None);
                None
            }
            Token::Doctype(_) => {
                self.error("unexpected-doctype");
                None
            }
            Token::StartTag(t) if t.name == "html" => self.in_body(Token::StartTag(t)),
            Token::StartTag(t) if t.name == "head" => {
                let node = self.insert_element(&t);
                self.head = Some(node);
                self.mode = Mode::InHead;
                None
            }
            Token::EndTag(t) if !matches!(t.name.as_str(), "head" | "body" | "html" | "br") => {
                self.error("unexpected-end-tag");
                None
            }
            other => self.before_head_anything_else(other),
        }
    }

    fn before_head_anything_else(&mut self, token: Token) -> Flow {
        let node = self.insert_element(&tag("head"));
        self.head = Some(node);
        self.mode = Mode::InHead;
        Some(token)
    }

    fn in_head(&mut self, token: Token) -> Flow {
        match token {
            Token::Characters(s) => {
                let (ws, rest) = split_ws(&s);
                self.insert_text(ws);
                if rest.is_empty() {
                    return None;
                }
                self.in_head_anything_else(Token::Characters(rest.to_owned()))
            }
            Token::Comment(c) => {
                self.insert_comment(c, None);
                None
            }
            Token::Doctype(_) => {
                self.error("unexpected-doctype");
                None
            }
            Token::StartTag(t) => match t.name.as_str() {
                "html" => self.in_body(Token::StartTag(t)),
                "base" | "basefont" | "bgsound" | "link" | "meta" => {
                    self.insert_void(&t);
                    None
                }
                "title" => {
                    self.generic_text_element(&t, TextMode::Rcdata);
                    None
                }
                "noscript" => {
                    // Scripting disabled: parse noscript content as markup.
                    self.insert_element(&t);
                    self.mode = Mode::InHeadNoscript;
                    None
                }
                "noframes" | "style" => {
                    self.generic_text_element(&t, TextMode::Rawtext);
                    None
                }
                "script" => {
                    self.generic_text_element(&t, TextMode::ScriptData);
                    None
                }
                "template" => {
                    // Unsupported template semantics: treat as an ordinary element.
                    self.insert_element(&t);
                    None
                }
                "head" => {
                    self.error("unexpected-start-tag");
                    None
                }
                _ => self.in_head_anything_else(Token::StartTag(t)),
            },
            Token::EndTag(t) => match t.name.as_str() {
                "head" => {
                    self.pop();
                    self.mode = Mode::AfterHead;
                    None
                }
                "body" | "html" | "br" => self.in_head_anything_else(Token::EndTag(t)),
                "template" if self.open.iter().any(|&n| self.name_of(n) == "template") => {
                    self.generate_all_implied_end_tags_thoroughly();
                    self.pop_until("template");
                    None
                }
                _ => {
                    self.error("unexpected-end-tag");
                    None
                }
            },
            Token::Eof => self.in_head_anything_else(Token::Eof),
        }
    }

    fn in_head_anything_else(&mut self, token: Token) -> Flow {
        self.pop();
        self.mode = Mode::AfterHead;
        Some(token)
    }

    fn in_head_noscript(&mut self, token: Token) -> Flow {
        match token {
            Token::Doctype(_) => None,
            Token::StartTag(t) if t.name == "html" => self.in_body(Token::StartTag(t)),
            Token::EndTag(t) if t.name == "noscript" => {
                self.pop();
                self.mode = Mode::InHead;
                None
            }
            Token::Characters(s) if is_all_ws(&s) => self.in_head(Token::Characters(s)),
            Token::Comment(c) => self.in_head(Token::Comment(c)),
            Token::StartTag(t)
                if matches!(
                    t.name.as_str(),
                    "basefont" | "bgsound" | "link" | "meta" | "noframes" | "style"
                ) =>
            {
                self.in_head(Token::StartTag(t))
            }
            Token::StartTag(t) if matches!(t.name.as_str(), "head" | "noscript") => {
                self.error("unexpected-start-tag");
                None
            }
            Token::EndTag(t) if t.name != "br" => {
                self.error("unexpected-end-tag");
                None
            }
            Token::Characters(s) => {
                let (ws, rest) = split_ws(&s);
                self.insert_text(ws);
                self.error("unexpected-character-in-noscript");
                self.pop();
                self.mode = Mode::InHead;
                Some(Token::Characters(rest.to_owned()))
            }
            other => {
                self.error("unexpected-token-in-noscript");
                self.pop();
                self.mode = Mode::InHead;
                Some(other)
            }
        }
    }

    fn after_head(&mut self, token: Token) -> Flow {
        match token {
            Token::Characters(s) => {
                let (ws, rest) = split_ws(&s);
                self.insert_text(ws);
                if rest.is_empty() {
                    return None;
                }
                self.after_head_anything_else(Token::Characters(rest.to_owned()))
            }
            Token::Comment(c) => {
                self.insert_comment(c, None);
                None
            }
            Token::Doctype(_) => None,
            Token::StartTag(t) => match t.name.as_str() {
                "html" => self.in_body(Token::StartTag(t)),
                "body" => {
                    self.insert_element(&t);
                    self.frameset_ok = false;
                    self.mode = Mode::InBody;
                    None
                }
                "frameset" => {
                    self.insert_element(&t);
                    self.mode = Mode::InFrameset;
                    None
                }
                "base" | "basefont" | "bgsound" | "link" | "meta" | "noframes" | "script"
                | "style" | "template" | "title" => {
                    self.error("unexpected-start-tag-after-head");
                    let Some(head) = self.head else {
                        return self.after_head_anything_else(Token::StartTag(t));
                    };
                    self.push_open(head);
                    let flow = self.in_head(Token::StartTag(t));
                    self.remove_from_stack(head);
                    flow
                }
                "head" => {
                    self.error("unexpected-start-tag");
                    None
                }
                _ => self.after_head_anything_else(Token::StartTag(t)),
            },
            Token::EndTag(t) => match t.name.as_str() {
                "template" => self.in_head(Token::EndTag(t)),
                "body" | "html" | "br" => self.after_head_anything_else(Token::EndTag(t)),
                _ => {
                    self.error("unexpected-end-tag");
                    None
                }
            },
            Token::Eof => self.after_head_anything_else(Token::Eof),
        }
    }

    fn after_head_anything_else(&mut self, token: Token) -> Flow {
        self.insert_element(&tag("body"));
        self.mode = Mode::InBody;
        Some(token)
    }

    fn in_body(&mut self, token: Token) -> Flow {
        match token {
            Token::Characters(s) => {
                self.reconstruct_formatting();
                self.insert_text(&s);
                if !is_all_ws(&s) {
                    self.frameset_ok = false;
                }
                None
            }
            Token::Comment(c) => {
                self.insert_comment(c, None);
                None
            }
            Token::Doctype(_) => {
                self.error("unexpected-doctype");
                None
            }
            Token::StartTag(t) => self.in_body_start(t),
            Token::EndTag(t) => self.in_body_end(t),
            Token::Eof => {
                self.stopped = true;
                None
            }
        }
    }

    fn in_body_start(&mut self, t: Tag) -> Flow {
        match t.name.as_str() {
            "html" => {
                self.error("unexpected-html-start-tag");
                if let Some(&html) = self.open.first() {
                    for a in t.attrs {
                        self.doc.add_attribute_if_missing(html, a);
                    }
                }
            }
            "base" | "basefont" | "bgsound" | "link" | "meta" | "noframes" | "script" | "style"
            | "template" | "title" => return self.in_head(Token::StartTag(t)),
            "body" => {
                self.error("unexpected-body-start-tag");
                if let Some(&body) = self.open.get(1)
                    && self.name_of(body) == "body"
                {
                    self.frameset_ok = false;
                    for a in t.attrs {
                        self.doc.add_attribute_if_missing(body, a);
                    }
                }
            }
            "frameset" => {
                self.error("unexpected-frameset-start-tag");
                let body = self.open.get(1).copied();
                if let Some(body) = body.filter(|&b| self.name_of(b) == "body" && self.frameset_ok)
                {
                    self.doc.detach(body);
                    self.truncate_open(1);
                    self.insert_element(&t);
                    self.mode = Mode::InFrameset;
                }
            }
            "address" | "article" | "aside" | "blockquote" | "center" | "details" | "dialog"
            | "dir" | "div" | "dl" | "fieldset" | "figcaption" | "figure" | "footer" | "header"
            | "hgroup" | "main" | "menu" | "nav" | "ol" | "p" | "search" | "section"
            | "summary" | "ul" => {
                self.close_p_if_in_button_scope();
                self.insert_element(&t);
            }
            name if HEADINGS.contains(&name) => {
                self.close_p_if_in_button_scope();
                if self.current_is_one_of(HEADINGS) {
                    self.error("nested-heading");
                    self.pop();
                }
                self.insert_element(&t);
            }
            "pre" | "listing" => {
                self.close_p_if_in_button_scope();
                self.insert_element(&t);
                self.ignore_lf = true;
                self.frameset_ok = false;
            }
            "form" => {
                if self.form.is_some() {
                    self.error("nested-form");
                } else {
                    self.close_p_if_in_button_scope();
                    let node = self.insert_element(&t);
                    self.form = Some(node);
                }
            }
            "li" | "dd" | "dt" => {
                self.frameset_ok = false;
                let closes: &[&str] = if t.name == "li" {
                    &["li"]
                } else {
                    &["dd", "dt"]
                };
                for i in (0..self.open.len()).rev() {
                    let node = self.open[i];
                    let name = self.name_of(node).to_owned();
                    if closes.contains(&name.as_str()) {
                        self.generate_implied_end_tags(Some(&name));
                        if !self.current_is(&name) {
                            self.error("unexpected-open-element");
                        }
                        self.pop_until(&name);
                        break;
                    }
                    if self.is_special(node) && !matches!(name.as_str(), "address" | "div" | "p") {
                        break;
                    }
                }
                self.close_p_if_in_button_scope();
                self.insert_element(&t);
            }
            "plaintext" => {
                self.close_p_if_in_button_scope();
                self.insert_element(&t);
                self.tokenizer.set_text_mode(TextMode::Plaintext);
            }
            "button" => {
                if self.in_scope("button", Scope::Default) {
                    self.error("nested-button");
                    self.generate_implied_end_tags(None);
                    self.pop_until("button");
                }
                self.reconstruct_formatting();
                self.insert_element(&t);
                self.frameset_ok = false;
            }
            "a" => {
                let existing = self
                    .formatting
                    .iter()
                    .rev()
                    .take_while(|e| !matches!(e, Formatting::Marker))
                    .find_map(|e| match e {
                        Formatting::Element(n, tg) if tg.name == "a" => Some(*n),
                        _ => None,
                    });
                if let Some(old) = existing {
                    self.error("nested-a");
                    self.adoption_agency("a");
                    if let Some(i) = self.formatting_index(old) {
                        self.formatting.remove(i);
                    }
                    self.remove_from_stack(old);
                }
                self.reconstruct_formatting();
                let node = self.insert_element(&t);
                self.push_formatting(node, &t);
            }
            "b" | "big" | "code" | "em" | "font" | "i" | "s" | "small" | "strike" | "strong"
            | "tt" | "u" => {
                self.reconstruct_formatting();
                let node = self.insert_element(&t);
                self.push_formatting(node, &t);
            }
            "nobr" => {
                self.reconstruct_formatting();
                if self.in_scope("nobr", Scope::Default) {
                    self.error("nested-nobr");
                    self.adoption_agency("nobr");
                    self.reconstruct_formatting();
                }
                let node = self.insert_element(&t);
                self.push_formatting(node, &t);
            }
            "applet" | "marquee" | "object" => {
                self.reconstruct_formatting();
                self.insert_element(&t);
                self.formatting.push(Formatting::Marker);
                self.frameset_ok = false;
            }
            "table" => {
                if self.doc.quirks_mode() != QuirksMode::Quirks {
                    self.close_p_if_in_button_scope();
                }
                self.insert_element(&t);
                self.frameset_ok = false;
                self.mode = Mode::InTable;
            }
            "area" | "br" | "embed" | "img" | "keygen" | "wbr" => {
                self.reconstruct_formatting();
                self.insert_void(&t);
                self.frameset_ok = false;
            }
            "input" => {
                self.reconstruct_formatting();
                self.insert_void(&t);
                if !t
                    .attr("type")
                    .is_some_and(|v| v.eq_ignore_ascii_case("hidden"))
                {
                    self.frameset_ok = false;
                }
            }
            "param" | "source" | "track" => self.insert_void(&t),
            "hr" => {
                self.close_p_if_in_button_scope();
                self.insert_void(&t);
                self.frameset_ok = false;
            }
            "image" => {
                self.error("image-start-tag");
                return Some(Token::StartTag(Tag {
                    name: "img".into(),
                    ..t
                }));
            }
            "textarea" => {
                self.insert_element(&t);
                self.ignore_lf = true;
                self.tokenizer.set_text_mode(TextMode::Rcdata);
                self.original_mode = self.mode;
                self.frameset_ok = false;
                self.mode = Mode::Text;
            }
            "xmp" => {
                self.close_p_if_in_button_scope();
                self.reconstruct_formatting();
                self.frameset_ok = false;
                self.generic_text_element(&t, TextMode::Rawtext);
            }
            "iframe" => {
                self.frameset_ok = false;
                self.generic_text_element(&t, TextMode::Rawtext);
            }
            "noembed" => self.generic_text_element(&t, TextMode::Rawtext),
            "select" => {
                self.reconstruct_formatting();
                self.insert_element(&t);
                self.frameset_ok = false;
                self.mode = if matches!(
                    self.mode,
                    Mode::InTable
                        | Mode::InCaption
                        | Mode::InTableBody
                        | Mode::InRow
                        | Mode::InCell
                ) {
                    Mode::InSelectInTable
                } else {
                    Mode::InSelect
                };
            }
            "optgroup" | "option" => {
                if self.current_is("option") {
                    self.pop();
                }
                self.reconstruct_formatting();
                self.insert_element(&t);
            }
            "rb" | "rtc" => {
                if self.in_scope("ruby", Scope::Default) {
                    self.generate_implied_end_tags(None);
                }
                self.insert_element(&t);
            }
            "rp" | "rt" => {
                if self.in_scope("ruby", Scope::Default) {
                    self.generate_implied_end_tags(Some("rtc"));
                }
                self.insert_element(&t);
            }
            "caption" | "col" | "colgroup" | "frame" | "head" | "tbody" | "td" | "tfoot" | "th"
            | "thead" | "tr" => {
                self.error("unexpected-start-tag-ignored");
            }
            _ => {
                // Includes math/svg, which are parsed as plain elements (no foreign content).
                self.reconstruct_formatting();
                self.insert_element(&t);
                if t.self_closing && matches!(t.name.as_str(), "math" | "svg") {
                    self.pop();
                }
            }
        }
        None
    }

    fn in_body_end(&mut self, t: Tag) -> Flow {
        match t.name.as_str() {
            "template" => return self.in_head(Token::EndTag(t)),
            "body" | "html" => {
                if !self.in_scope("body", Scope::Default) {
                    self.error("unexpected-end-tag");
                    return None;
                }
                self.mode = Mode::AfterBody;
                if t.name == "html" {
                    return Some(Token::EndTag(t));
                }
            }
            "address" | "article" | "aside" | "blockquote" | "button" | "center" | "details"
            | "dialog" | "dir" | "div" | "dl" | "fieldset" | "figcaption" | "figure" | "footer"
            | "header" | "hgroup" | "listing" | "main" | "menu" | "nav" | "ol" | "pre"
            | "search" | "section" | "summary" | "ul" => {
                if !self.in_scope(&t.name, Scope::Default) {
                    self.error("unexpected-end-tag");
                    return None;
                }
                self.generate_implied_end_tags(None);
                if !self.current_is(&t.name) {
                    self.error("end-tag-too-early");
                }
                self.pop_until(&t.name);
            }
            "form" => {
                let node = self.form.take();
                match node.filter(|&n| self.in_scope_node_default(n)) {
                    None => self.error("unexpected-end-tag"),
                    Some(node) => {
                        self.generate_implied_end_tags(None);
                        if self.current() != Some(node) {
                            self.error("end-tag-too-early");
                        }
                        self.remove_from_stack(node);
                    }
                }
            }
            "p" => {
                if !self.in_scope("p", Scope::Button) {
                    self.error("unexpected-end-tag-p");
                    self.insert_element(&tag("p"));
                }
                self.close_p();
            }
            "li" => {
                if !self.in_scope("li", Scope::ListItem) {
                    self.error("unexpected-end-tag");
                    return None;
                }
                self.generate_implied_end_tags(Some("li"));
                self.pop_until("li");
            }
            "dd" | "dt" => {
                if !self.in_scope(&t.name, Scope::Default) {
                    self.error("unexpected-end-tag");
                    return None;
                }
                self.generate_implied_end_tags(Some(&t.name));
                self.pop_until(&t.name);
            }
            name if HEADINGS.contains(&name) => {
                if !self.in_scope_any(HEADINGS, Scope::Default) {
                    self.error("unexpected-end-tag");
                    return None;
                }
                self.generate_implied_end_tags(None);
                self.pop_until_one_of(HEADINGS);
            }
            name if FORMATTING.contains(&name) => {
                if self.adoption_agency(name) {
                    self.any_other_end_tag(&t.name);
                }
            }
            "applet" | "marquee" | "object" => {
                if !self.in_scope(&t.name, Scope::Default) {
                    self.error("unexpected-end-tag");
                    return None;
                }
                self.generate_implied_end_tags(None);
                self.pop_until(&t.name);
                self.clear_formatting_to_marker();
            }
            "br" => {
                self.error("unexpected-br-end-tag");
                return self.in_body_start(tag("br"));
            }
            _ => self.any_other_end_tag(&t.name),
        }
        None
    }

    fn in_scope_node_default(&self, node: NodeId) -> bool {
        self.in_scope_node(node)
    }

    fn any_other_end_tag(&mut self, name: &str) {
        for i in (0..self.open.len()).rev() {
            let node = self.open[i];
            if self.name_of(node) == name {
                self.generate_implied_end_tags(Some(name));
                self.truncate_open(i);
                return;
            }
            if self.is_special(node) {
                self.error("unexpected-end-tag");
                return;
            }
        }
    }

    fn text(&mut self, token: Token) -> Flow {
        match token {
            Token::Characters(s) => self.insert_text(&s),
            Token::Eof => {
                self.error("eof-in-text");
                self.pop();
                self.mode = self.original_mode;
                return Some(Token::Eof);
            }
            Token::EndTag(_) => {
                self.pop();
                self.mode = self.original_mode;
            }
            _ => {}
        }
        None
    }

    fn in_table(&mut self, token: Token) -> Flow {
        match token {
            Token::Characters(s)
                if self
                    .current_is_one_of(&["table", "tbody", "template", "tfoot", "thead", "tr"]) =>
            {
                self.pending_table_text.clear();
                self.original_mode = self.mode;
                self.mode = Mode::InTableText;
                Some(Token::Characters(s))
            }
            Token::Comment(c) => {
                self.insert_comment(c, None);
                None
            }
            Token::Doctype(_) => None,
            Token::StartTag(t) => match t.name.as_str() {
                "caption" => {
                    self.clear_to_table_context();
                    self.formatting.push(Formatting::Marker);
                    self.insert_element(&t);
                    self.mode = Mode::InCaption;
                    None
                }
                "colgroup" => {
                    self.clear_to_table_context();
                    self.insert_element(&t);
                    self.mode = Mode::InColumnGroup;
                    None
                }
                "col" => {
                    self.clear_to_table_context();
                    self.insert_element(&tag("colgroup"));
                    self.mode = Mode::InColumnGroup;
                    Some(Token::StartTag(t))
                }
                "tbody" | "tfoot" | "thead" => {
                    self.clear_to_table_context();
                    self.insert_element(&t);
                    self.mode = Mode::InTableBody;
                    None
                }
                "td" | "th" | "tr" => {
                    self.clear_to_table_context();
                    self.insert_element(&tag("tbody"));
                    self.mode = Mode::InTableBody;
                    Some(Token::StartTag(t))
                }
                "table" => {
                    self.error("nested-table");
                    if !self.in_scope("table", Scope::Table) {
                        return None;
                    }
                    self.pop_until("table");
                    self.reset_insertion_mode();
                    Some(Token::StartTag(t))
                }
                "style" | "script" | "template" => self.in_head(Token::StartTag(t)),
                "input"
                    if t.attr("type")
                        .is_some_and(|v| v.eq_ignore_ascii_case("hidden")) =>
                {
                    self.error("input-in-table");
                    self.insert_void(&t);
                    None
                }
                "form" => {
                    self.error("form-in-table");
                    if self.form.is_none() {
                        let node = self.insert_element(&t);
                        self.form = Some(node);
                        self.pop();
                    }
                    None
                }
                _ => self.in_table_anything_else(Token::StartTag(t)),
            },
            Token::EndTag(t) => match t.name.as_str() {
                "table" => {
                    if !self.in_scope("table", Scope::Table) {
                        self.error("unexpected-end-tag");
                        return None;
                    }
                    self.pop_until("table");
                    self.reset_insertion_mode();
                    None
                }
                "body" | "caption" | "col" | "colgroup" | "html" | "tbody" | "td" | "tfoot"
                | "th" | "thead" | "tr" => {
                    self.error("unexpected-end-tag");
                    None
                }
                "template" => self.in_head(Token::EndTag(t)),
                _ => self.in_table_anything_else(Token::EndTag(t)),
            },
            Token::Eof => self.in_body(Token::Eof),
            other => self.in_table_anything_else(other),
        }
    }

    fn in_table_anything_else(&mut self, token: Token) -> Flow {
        self.error("foster-parented-content");
        self.foster_parenting = true;
        let flow = self.in_body(token);
        self.foster_parenting = false;
        flow
    }

    fn in_table_text(&mut self, token: Token) -> Flow {
        if let Token::Characters(s) = &token {
            self.pending_table_text.push_str(s);
            return None;
        }
        let pending = std::mem::take(&mut self.pending_table_text);
        if !is_all_ws(&pending) {
            self.error("non-whitespace-in-table");
            self.foster_parenting = true;
            self.in_body(Token::Characters(pending));
            self.foster_parenting = false;
        } else {
            self.insert_text(&pending);
        }
        self.mode = self.original_mode;
        Some(token)
    }

    fn close_caption(&mut self) -> bool {
        if !self.in_scope("caption", Scope::Table) {
            self.error("unexpected-end-tag");
            return false;
        }
        self.generate_implied_end_tags(None);
        self.pop_until("caption");
        self.clear_formatting_to_marker();
        self.mode = Mode::InTable;
        true
    }

    fn in_caption(&mut self, token: Token) -> Flow {
        match &token {
            Token::EndTag(t) if t.name == "caption" => {
                self.close_caption();
                None
            }
            Token::StartTag(t)
                if matches!(
                    t.name.as_str(),
                    "caption"
                        | "col"
                        | "colgroup"
                        | "tbody"
                        | "td"
                        | "tfoot"
                        | "th"
                        | "thead"
                        | "tr"
                ) =>
            {
                self.close_caption().then_some(token)
            }
            Token::EndTag(t) if t.name == "table" => self.close_caption().then_some(token),
            Token::EndTag(t)
                if matches!(
                    t.name.as_str(),
                    "body"
                        | "col"
                        | "colgroup"
                        | "html"
                        | "tbody"
                        | "td"
                        | "tfoot"
                        | "th"
                        | "thead"
                        | "tr"
                ) =>
            {
                self.error("unexpected-end-tag");
                None
            }
            _ => self.in_body(token),
        }
    }

    fn in_column_group(&mut self, token: Token) -> Flow {
        match token {
            Token::Characters(s) => {
                let (ws, rest) = split_ws(&s);
                self.insert_text(ws);
                if rest.is_empty() {
                    return None;
                }
                self.in_column_group_anything_else(Token::Characters(rest.to_owned()))
            }
            Token::Comment(c) => {
                self.insert_comment(c, None);
                None
            }
            Token::Doctype(_) => None,
            Token::StartTag(t) if t.name == "html" => self.in_body(Token::StartTag(t)),
            Token::StartTag(t) if t.name == "col" => {
                self.insert_void(&t);
                None
            }
            Token::EndTag(t) if t.name == "colgroup" => {
                if self.current_is("colgroup") {
                    self.pop();
                    self.mode = Mode::InTable;
                } else {
                    self.error("unexpected-end-tag");
                }
                None
            }
            Token::EndTag(t) if t.name == "col" => {
                self.error("unexpected-end-tag");
                None
            }
            Token::StartTag(t) if t.name == "template" => self.in_head(Token::StartTag(t)),
            Token::EndTag(t) if t.name == "template" => self.in_head(Token::EndTag(t)),
            Token::Eof => self.in_body(Token::Eof),
            other => self.in_column_group_anything_else(other),
        }
    }

    fn in_column_group_anything_else(&mut self, token: Token) -> Flow {
        if !self.current_is("colgroup") {
            self.error("unexpected-token-in-colgroup");
            return None;
        }
        self.pop();
        self.mode = Mode::InTable;
        Some(token)
    }

    fn in_table_body(&mut self, token: Token) -> Flow {
        match &token {
            Token::StartTag(t) if t.name == "tr" => {
                self.clear_to_table_body_context();
                self.insert_element(t);
                self.mode = Mode::InRow;
                None
            }
            Token::StartTag(t) if matches!(t.name.as_str(), "th" | "td") => {
                self.error("cell-outside-row");
                self.clear_to_table_body_context();
                self.insert_element(&tag("tr"));
                self.mode = Mode::InRow;
                Some(token)
            }
            Token::EndTag(t) if matches!(t.name.as_str(), "tbody" | "tfoot" | "thead") => {
                if !self.in_scope(&t.name, Scope::Table) {
                    self.error("unexpected-end-tag");
                    return None;
                }
                self.clear_to_table_body_context();
                self.pop();
                self.mode = Mode::InTable;
                None
            }
            Token::StartTag(t)
                if matches!(
                    t.name.as_str(),
                    "caption" | "col" | "colgroup" | "tbody" | "tfoot" | "thead"
                ) =>
            {
                self.leave_table_body(token)
            }
            Token::EndTag(t) if t.name == "table" => self.leave_table_body(token),
            Token::EndTag(t)
                if matches!(
                    t.name.as_str(),
                    "body" | "caption" | "col" | "colgroup" | "html" | "td" | "th" | "tr"
                ) =>
            {
                self.error("unexpected-end-tag");
                None
            }
            _ => self.in_table(token),
        }
    }

    fn leave_table_body(&mut self, token: Token) -> Flow {
        if !self.in_scope_any(&["tbody", "thead", "tfoot"], Scope::Table) {
            self.error("unexpected-token");
            return None;
        }
        self.clear_to_table_body_context();
        self.pop();
        self.mode = Mode::InTable;
        Some(token)
    }

    fn close_row(&mut self) -> bool {
        if !self.in_scope("tr", Scope::Table) {
            self.error("unexpected-token");
            return false;
        }
        self.clear_to_row_context();
        self.pop();
        self.mode = Mode::InTableBody;
        true
    }

    fn in_row(&mut self, token: Token) -> Flow {
        match &token {
            Token::StartTag(t) if matches!(t.name.as_str(), "th" | "td") => {
                self.clear_to_row_context();
                self.insert_element(t);
                self.mode = Mode::InCell;
                self.formatting.push(Formatting::Marker);
                None
            }
            Token::EndTag(t) if t.name == "tr" => {
                self.close_row();
                None
            }
            Token::StartTag(t)
                if matches!(
                    t.name.as_str(),
                    "caption" | "col" | "colgroup" | "tbody" | "tfoot" | "thead" | "tr"
                ) =>
            {
                self.close_row().then_some(token)
            }
            Token::EndTag(t) if t.name == "table" => self.close_row().then_some(token),
            Token::EndTag(t) if matches!(t.name.as_str(), "tbody" | "tfoot" | "thead") => {
                if !self.in_scope(&t.name, Scope::Table) {
                    self.error("unexpected-end-tag");
                    return None;
                }
                self.close_row().then_some(token)
            }
            Token::EndTag(t)
                if matches!(
                    t.name.as_str(),
                    "body" | "caption" | "col" | "colgroup" | "html" | "td" | "th"
                ) =>
            {
                self.error("unexpected-end-tag");
                None
            }
            _ => self.in_table(token),
        }
    }

    fn close_cell(&mut self) {
        self.generate_implied_end_tags(None);
        if !self.current_is_one_of(&["td", "th"]) {
            self.error("unclosed-cell-content");
        }
        self.pop_until_one_of(&["td", "th"]);
        self.clear_formatting_to_marker();
        self.mode = Mode::InRow;
    }

    fn in_cell(&mut self, token: Token) -> Flow {
        match &token {
            Token::EndTag(t) if matches!(t.name.as_str(), "td" | "th") => {
                if !self.in_scope(&t.name, Scope::Table) {
                    self.error("unexpected-end-tag");
                    return None;
                }
                self.generate_implied_end_tags(None);
                if !self.current_is(&t.name) {
                    self.error("end-tag-too-early");
                }
                self.pop_until(&t.name);
                self.clear_formatting_to_marker();
                self.mode = Mode::InRow;
                None
            }
            Token::StartTag(t)
                if matches!(
                    t.name.as_str(),
                    "caption"
                        | "col"
                        | "colgroup"
                        | "tbody"
                        | "td"
                        | "tfoot"
                        | "th"
                        | "thead"
                        | "tr"
                ) =>
            {
                if !self.in_scope_any(&["td", "th"], Scope::Table) {
                    self.error("unexpected-start-tag");
                    return None;
                }
                self.close_cell();
                Some(token)
            }
            Token::EndTag(t)
                if matches!(
                    t.name.as_str(),
                    "body" | "caption" | "col" | "colgroup" | "html"
                ) =>
            {
                self.error("unexpected-end-tag");
                None
            }
            Token::EndTag(t)
                if matches!(
                    t.name.as_str(),
                    "table" | "tbody" | "tfoot" | "thead" | "tr"
                ) =>
            {
                if !self.in_scope(&t.name, Scope::Table) {
                    self.error("unexpected-end-tag");
                    return None;
                }
                self.close_cell();
                Some(token)
            }
            _ => self.in_body(token),
        }
    }

    fn in_select(&mut self, token: Token) -> Flow {
        match token {
            Token::Characters(s) => {
                self.insert_text(&s);
                None
            }
            Token::Comment(c) => {
                self.insert_comment(c, None);
                None
            }
            Token::Doctype(_) => None,
            Token::StartTag(t) => match t.name.as_str() {
                "html" => self.in_body(Token::StartTag(t)),
                "option" => {
                    if self.current_is("option") {
                        self.pop();
                    }
                    self.insert_element(&t);
                    None
                }
                "optgroup" => {
                    if self.current_is("option") {
                        self.pop();
                    }
                    if self.current_is("optgroup") {
                        self.pop();
                    }
                    self.insert_element(&t);
                    None
                }
                "hr" => {
                    if self.current_is("option") {
                        self.pop();
                    }
                    if self.current_is("optgroup") {
                        self.pop();
                    }
                    self.insert_void(&t);
                    None
                }
                "select" => {
                    self.error("nested-select");
                    if self.in_scope("select", Scope::Select) {
                        self.pop_until("select");
                        self.reset_insertion_mode();
                    }
                    None
                }
                "input" | "keygen" | "textarea" => {
                    self.error("unexpected-start-tag-in-select");
                    if !self.in_scope("select", Scope::Select) {
                        return None;
                    }
                    self.pop_until("select");
                    self.reset_insertion_mode();
                    Some(Token::StartTag(t))
                }
                "script" | "template" => self.in_head(Token::StartTag(t)),
                _ => {
                    self.error("unexpected-start-tag-in-select");
                    None
                }
            },
            Token::EndTag(t) => match t.name.as_str() {
                "optgroup" => {
                    let n = self.open.len();
                    if self.current_is("option")
                        && n >= 2
                        && self.name_of(self.open[n - 2]) == "optgroup"
                    {
                        self.pop();
                    }
                    if self.current_is("optgroup") {
                        self.pop();
                    } else {
                        self.error("unexpected-end-tag");
                    }
                    None
                }
                "option" => {
                    if self.current_is("option") {
                        self.pop();
                    } else {
                        self.error("unexpected-end-tag");
                    }
                    None
                }
                "select" => {
                    if self.in_scope("select", Scope::Select) {
                        self.pop_until("select");
                        self.reset_insertion_mode();
                    } else {
                        self.error("unexpected-end-tag");
                    }
                    None
                }
                "template" => self.in_head(Token::EndTag(t)),
                _ => {
                    self.error("unexpected-end-tag-in-select");
                    None
                }
            },
            Token::Eof => self.in_body(Token::Eof),
        }
    }

    fn in_select_in_table(&mut self, token: Token) -> Flow {
        const TABLE_TAGS: &[&str] = &[
            "caption", "table", "tbody", "tfoot", "thead", "tr", "td", "th",
        ];
        match &token {
            Token::StartTag(t) if TABLE_TAGS.contains(&t.name.as_str()) => {
                self.error("table-tag-in-select");
                self.pop_until("select");
                self.reset_insertion_mode();
                Some(token)
            }
            Token::EndTag(t) if TABLE_TAGS.contains(&t.name.as_str()) => {
                self.error("table-tag-in-select");
                if !self.in_scope(&t.name, Scope::Table) {
                    return None;
                }
                self.pop_until("select");
                self.reset_insertion_mode();
                Some(token)
            }
            _ => self.in_select(token),
        }
    }

    fn after_body(&mut self, token: Token) -> Flow {
        match token {
            Token::Characters(s) => {
                let (ws, rest) = split_ws(&s);
                if !ws.is_empty() {
                    self.in_body(Token::Characters(ws.to_owned()));
                }
                if rest.is_empty() {
                    return None;
                }
                self.error("content-after-body");
                self.mode = Mode::InBody;
                Some(Token::Characters(rest.to_owned()))
            }
            Token::Comment(c) => {
                let html = self.open.first().copied();
                self.insert_comment(c, html);
                None
            }
            Token::Doctype(_) => None,
            Token::StartTag(t) if t.name == "html" => self.in_body(Token::StartTag(t)),
            Token::EndTag(t) if t.name == "html" => {
                self.mode = Mode::AfterAfterBody;
                None
            }
            Token::Eof => {
                self.stopped = true;
                None
            }
            other => {
                self.error("content-after-body");
                self.mode = Mode::InBody;
                Some(other)
            }
        }
    }

    fn in_frameset(&mut self, token: Token) -> Flow {
        match token {
            Token::Characters(s) => {
                let ws: String = s.chars().filter(|&c| is_ws_char(c)).collect();
                self.insert_text(&ws);
            }
            Token::Comment(c) => self.insert_comment(c, None),
            Token::StartTag(t) => match t.name.as_str() {
                "html" => return self.in_body(Token::StartTag(t)),
                "frameset" => {
                    self.insert_element(&t);
                }
                "frame" => self.insert_void(&t),
                "noframes" => return self.in_head(Token::StartTag(t)),
                _ => self.error("unexpected-start-tag-in-frameset"),
            },
            Token::EndTag(t) if t.name == "frameset" => {
                if self.open.len() <= 1 {
                    self.error("unexpected-end-tag");
                } else {
                    self.pop();
                    if !self.current_is("frameset") {
                        self.mode = Mode::AfterFrameset;
                    }
                }
            }
            Token::Eof => self.stopped = true,
            _ => self.error("unexpected-token-in-frameset"),
        }
        None
    }

    fn after_frameset(&mut self, token: Token) -> Flow {
        match token {
            Token::Characters(s) => {
                let ws: String = s.chars().filter(|&c| is_ws_char(c)).collect();
                self.insert_text(&ws);
            }
            Token::Comment(c) => self.insert_comment(c, None),
            Token::StartTag(t) if t.name == "html" => return self.in_body(Token::StartTag(t)),
            Token::StartTag(t) if t.name == "noframes" => return self.in_head(Token::StartTag(t)),
            Token::EndTag(t) if t.name == "html" => self.mode = Mode::AfterAfterFrameset,
            Token::Eof => self.stopped = true,
            _ => self.error("unexpected-token-after-frameset"),
        }
        None
    }

    fn after_after_body(&mut self, token: Token) -> Flow {
        match token {
            Token::Comment(c) => {
                let root = self.doc.root();
                self.insert_comment(c, Some(root));
                None
            }
            Token::Doctype(_) => self.in_body(token),
            Token::StartTag(t) if t.name == "html" => self.in_body(Token::StartTag(t)),
            Token::Characters(s) => {
                let (ws, rest) = split_ws(&s);
                if !ws.is_empty() {
                    self.in_body(Token::Characters(ws.to_owned()));
                }
                if rest.is_empty() {
                    return None;
                }
                self.error("content-after-html");
                self.mode = Mode::InBody;
                Some(Token::Characters(rest.to_owned()))
            }
            Token::Eof => {
                self.stopped = true;
                None
            }
            other => {
                self.error("content-after-html");
                self.mode = Mode::InBody;
                Some(other)
            }
        }
    }

    fn after_after_frameset(&mut self, token: Token) -> Flow {
        match token {
            Token::Comment(c) => {
                let root = self.doc.root();
                self.insert_comment(c, Some(root));
                None
            }
            Token::Doctype(_) => self.in_body(token),
            Token::StartTag(t) if t.name == "html" => self.in_body(Token::StartTag(t)),
            Token::Characters(s) => {
                let ws: String = s.chars().filter(|&c| is_ws_char(c)).collect();
                if !ws.is_empty() {
                    self.in_body(Token::Characters(ws));
                }
                None
            }
            Token::StartTag(t) if t.name == "noframes" => self.in_head(Token::StartTag(t)),
            Token::Eof => {
                self.stopped = true;
                None
            }
            _ => {
                self.error("unexpected-token-after-frameset");
                None
            }
        }
    }
}

fn same_attributes(a: &Tag, b: &Tag) -> bool {
    a.attrs.len() == b.attrs.len()
        && a.attrs.iter().all(|x| {
            b.attrs
                .iter()
                .any(|y| y.name == x.name && y.value == x.value)
        })
}

/// Determines the document mode from a DOCTYPE token (§13.2.6.4.1).
fn quirks_mode_for(d: &Doctype) -> QuirksMode {
    const QUIRKS_PUBLIC_PREFIXES: &[&str] = &[
        "+//silmaril//dtd html pro v0r11 19970101//",
        "-//as//dtd html 3.0 aswedit + extensions//",
        "-//advasoft ltd//dtd html 3.0 aswedit + extensions//",
        "-//ietf//dtd html",
        "-//metrius//dtd metrius presentational//",
        "-//microsoft//dtd internet explorer 2.0 html strict//",
        "-//microsoft//dtd internet explorer 2.0 html//",
        "-//microsoft//dtd internet explorer 2.0 tables//",
        "-//microsoft//dtd internet explorer 3.0 html strict//",
        "-//microsoft//dtd internet explorer 3.0 html//",
        "-//microsoft//dtd internet explorer 3.0 tables//",
        "-//netscape comm. corp.//dtd html//",
        "-//netscape comm. corp.//dtd strict html//",
        "-//o'reilly and associates//dtd html 2.0//",
        "-//o'reilly and associates//dtd html extended 1.0//",
        "-//o'reilly and associates//dtd html extended relaxed 1.0//",
        "-//sq//dtd html 2.0 hotmetal + extensions//",
        "-//softquad software//dtd hotmetal pro 6.0::19990601::extensions to html 4.0//",
        "-//softquad//dtd hotmetal pro 4.0::19971010::extensions to html 4.0//",
        "-//spyglass//dtd html 2.0 extended//",
        "-//sun microsystems corp.//dtd hotjava html//",
        "-//sun microsystems corp.//dtd hotjava strict html//",
        "-//w3c//dtd html 3 1995-03-24//",
        "-//w3c//dtd html 3.2 draft//",
        "-//w3c//dtd html 3.2 final//",
        "-//w3c//dtd html 3.2//",
        "-//w3c//dtd html 3.2s draft//",
        "-//w3c//dtd html 4.0 frameset//",
        "-//w3c//dtd html 4.0 transitional//",
        "-//w3c//dtd html experimental 19960712//",
        "-//w3c//dtd html experimental 970421//",
        "-//w3c//dtd w3 html//",
        "-//w3o//dtd w3 html 3.0//",
        "-//webtechs//dtd mozilla html 2.0//",
        "-//webtechs//dtd mozilla html//",
    ];
    const QUIRKS_PUBLIC_EXACT: &[&str] = &[
        "-//w3o//dtd w3 html strict 3.0//en//",
        "-/w3c/dtd html 4.0 transitional/en",
        "html",
    ];
    const HTML401_PREFIXES: &[&str] = &[
        "-//w3c//dtd html 4.01 frameset//",
        "-//w3c//dtd html 4.01 transitional//",
    ];
    const XHTML_LIMITED_PREFIXES: &[&str] = &[
        "-//w3c//dtd xhtml 1.0 frameset//",
        "-//w3c//dtd xhtml 1.0 transitional//",
    ];

    if d.force_quirks || d.name.as_deref() != Some("html") {
        return QuirksMode::Quirks;
    }
    let public = d.public_id.as_deref().map(str::to_ascii_lowercase);
    let system = d.system_id.as_deref().map(str::to_ascii_lowercase);
    let starts = |prefixes: &[&str]| {
        public
            .as_deref()
            .is_some_and(|p| prefixes.iter().any(|pre| p.starts_with(pre)))
    };
    if public
        .as_deref()
        .is_some_and(|p| QUIRKS_PUBLIC_EXACT.contains(&p))
        || system.as_deref() == Some("http://www.ibm.com/data/dtd/v11/ibmxhtml1-transitional.dtd")
        || starts(QUIRKS_PUBLIC_PREFIXES)
        || (system.is_none() && starts(HTML401_PREFIXES))
    {
        return QuirksMode::Quirks;
    }
    if starts(XHTML_LIMITED_PREFIXES) || (system.is_some() && starts(HTML401_PREFIXES)) {
        return QuirksMode::LimitedQuirks;
    }
    QuirksMode::NoQuirks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(doctype: &str) -> QuirksMode {
        parse_document(&format!("{doctype}<p>x"))
            .document
            .quirks_mode()
    }

    #[test]
    fn quirks_mode_detection() {
        assert_eq!(mode("<!DOCTYPE html>"), QuirksMode::NoQuirks);
        assert_eq!(mode(""), QuirksMode::Quirks);
        assert_eq!(
            mode(r#"<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01 Transitional//EN">"#),
            QuirksMode::Quirks
        );
        assert_eq!(
            mode(
                r#"<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01 Transitional//EN" "http://www.w3.org/TR/html4/loose.dtd">"#
            ),
            QuirksMode::LimitedQuirks
        );
        assert_eq!(
            mode(r#"<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Strict//EN" "x">"#),
            QuirksMode::NoQuirks
        );
        assert_eq!(mode("<!DOCTYPE foo>"), QuirksMode::Quirks);
    }

    #[test]
    fn deeply_nested_markup_is_linear_and_safe() {
        let input = "<div>".repeat(100_000);
        let out = parse_document(&input);
        let divs = out
            .document
            .descendants(out.document.root())
            .filter(|&n| out.document.is_element_named(n, "div"))
            .count();
        assert_eq!(divs, 100_000);
    }

    #[test]
    fn many_misnested_formatting_elements_terminate() {
        let input = "<b><i><u><s>".repeat(2_000) + &"</b>x".repeat(2_000);
        let out = parse_document(&input);
        assert!(out.error_count > 0);
    }
}
