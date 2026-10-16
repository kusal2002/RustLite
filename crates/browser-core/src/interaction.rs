//! User interaction with page content: clicks, focus, keyboard editing of
//! form controls, and form submission (HTML §6.6 focus, §4.10 forms,
//! UI Events for keyboard).
//!
//! Order of operations, as in browsers: the event goes to page scripts
//! first; only if no listener calls `preventDefault()` does the default
//! action run (toggle a checkbox, focus a field, edit text, submit a form).

use rustlite_dom::{ControlKind, Document, NodeId};
use rustlite_net::Url;
use rustlite_script::EventInit;

use crate::{Content, Page};

/// A key press delivered to the page (independent of the windowing system).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageKey {
    /// Text typed (one or more characters, no control characters).
    Text(String),
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
    Enter,
    Tab {
        shift: bool,
    },
    Escape,
    /// Any other key, by its DOM `key` name (e.g. "ArrowUp", "F2").
    Other(String),
}

impl PageKey {
    /// The DOM `KeyboardEvent.key` value.
    pub fn dom_key(&self) -> String {
        match self {
            PageKey::Text(t) => t.clone(),
            PageKey::Backspace => "Backspace".into(),
            PageKey::Delete => "Delete".into(),
            PageKey::Left => "ArrowLeft".into(),
            PageKey::Right => "ArrowRight".into(),
            PageKey::Home => "Home".into(),
            PageKey::End => "End".into(),
            PageKey::Enter => "Enter".into(),
            PageKey::Tab { .. } => "Tab".into(),
            PageKey::Escape => "Escape".into(),
            PageKey::Other(k) => k.clone(),
        }
    }
}

/// What the browser should do after the page handled input.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputOutcome {
    /// The page used the input (a listener cancelled it, or a control acted
    /// on it); the browser must not apply its own default (scrolling, link
    /// navigation).
    pub consumed: bool,
    /// A form was submitted: navigate here.
    pub navigate: Option<Url>,
}

/// Elements that can take focus by Tab, in tree order (controls only; no
/// `tabindex` support yet).
fn focusable(doc: &Document) -> Vec<NodeId> {
    doc.descendants(doc.root())
        .filter(|&n| doc.control_kind(n).is_some() && doc.attribute(n, "disabled").is_none())
        .collect()
}

fn form_of(doc: &Document, n: NodeId) -> Option<NodeId> {
    doc.ancestors(n).find(|&a| doc.is_element_named(a, "form"))
}

/// Is `n` a submit button (`<button>` without `type=button|reset`, or
/// `<input type=submit|image>`)?
fn is_submit_button(doc: &Document, n: NodeId) -> bool {
    let t = doc
        .attribute(n, "type")
        .map(|t| t.trim().to_ascii_lowercase())
        .unwrap_or_default();
    match doc.tag_name(n) {
        Some("button") => !matches!(t.as_str(), "button" | "reset"),
        Some("input") => matches!(t.as_str(), "submit" | "image"),
        _ => false,
    }
}

impl Page {
    fn html_mut(&mut self) -> Option<&mut crate::HtmlDocument> {
        match &mut self.content {
            Content::Html(doc) => Some(doc),
            _ => None,
        }
    }

    /// Dispatches `kind` at `target` to page scripts, if any. Returns
    /// whether it was cancelled. Script changes restyle the page.
    fn fire(&mut self, target: NodeId, kind: &str, init: &EventInit) -> bool {
        let Some(doc) = self.html_mut() else {
            return false;
        };
        let Some(host) = doc.scripts.host.as_mut() else {
            return false;
        };
        let out = host.dispatch_event_init(&mut doc.dom, target, kind, init);
        for e in &out.errors {
            tracing::warn!(target: "console", "uncaught {e}");
        }
        if out.mutated {
            self.document_mutated();
        }
        out.default_prevented
    }

    /// Form state changed (value, checkedness, focus): lay out and paint
    /// again; styles are unaffected.
    fn form_changed(&mut self) {
        if let Some(doc) = self.html_mut() {
            doc.layout = None;
        }
        self.paint_dirty = true;
    }

    /// The focused element, if any.
    pub fn focused(&self) -> Option<NodeId> {
        match &self.content {
            Content::Html(doc) => doc.dom.focused(),
            _ => None,
        }
    }

    /// Moves focus, firing `change` (if the field's value changed while
    /// focused), `blur` and `focus`.
    fn set_focus(&mut self, target: Option<NodeId>) {
        let Some(doc) = self.html_mut() else { return };
        let previous = doc.dom.focused();
        if previous == target {
            return;
        }
        doc.dom.set_focused(target);
        let previous_value = previous.map(|p| doc.dom.control_value(p));
        let changed = previous_value
            .zip(self.focus_value.as_ref())
            .is_some_and(|(now, before)| now != *before);
        self.focus_value = target.map(|t| self.html_dom().control_value(t));
        self.caret = target.map_or(0, |t| self.html_dom().control_value(t).chars().count());
        self.form_changed();
        let no_bubble = EventInit {
            no_bubble: true,
            key: None,
        };
        if let Some(p) = previous {
            if changed {
                self.fire(p, "change", &EventInit::default());
            }
            self.fire(p, "blur", &no_bubble);
        }
        if let Some(t) = target {
            self.fire(t, "focus", &no_bubble);
        }
    }

    fn html_dom(&self) -> &Document {
        match &self.content {
            Content::Html(doc) => &doc.dom,
            _ => unreachable!("only called with an HTML document"),
        }
    }

    /// A primary-button click at a point of the page viewport (device px):
    /// scripts first, then the default action for form controls.
    pub fn click(&mut self, x: f32, y: f32) -> InputOutcome {
        let (scale, scroll) = (self.device_scale, self.scroll_y);
        let target = self
            .layout()
            .and_then(|(_, tree)| rustlite_layout::hit_test(tree, x / scale, y / scale + scroll));
        let Some(target) = target else {
            self.set_focus(None);
            return InputOutcome::default();
        };
        tracing::debug!(?target, x, y, "page click");
        if self.fire(target, "click", &EventInit::default()) {
            return InputOutcome {
                consumed: true,
                navigate: None,
            };
        }
        let kind = self.html_mut().and_then(|d| d.dom.control_kind(target));
        let disabled = self
            .html_mut()
            .is_some_and(|d| d.dom.attribute(target, "disabled").is_some());
        match kind {
            Some(_) if disabled => InputOutcome {
                consumed: true,
                navigate: None,
            },
            Some(ControlKind::Checkbox | ControlKind::Radio) => {
                self.set_focus(Some(target));
                let doc = self.html_mut().expect("has a control");
                let was = doc.dom.checked(target);
                let radio = doc.dom.control_kind(target) == Some(ControlKind::Radio);
                if radio && was {
                    return InputOutcome {
                        consumed: true,
                        navigate: None,
                    };
                }
                if radio {
                    rustlite_script::uncheck_radio_group(&mut doc.dom, target);
                }
                doc.dom.set_checked(target, !was);
                self.form_changed();
                self.fire(target, "input", &EventInit::default());
                self.fire(target, "change", &EventInit::default());
                InputOutcome {
                    consumed: true,
                    navigate: None,
                }
            }
            Some(ControlKind::Text { .. }) => {
                self.set_focus(Some(target));
                InputOutcome {
                    consumed: true,
                    navigate: None,
                }
            }
            Some(ControlKind::Button) => {
                self.set_focus(Some(target));
                let submits = self
                    .html_mut()
                    .is_some_and(|d| is_submit_button(&d.dom, target));
                let form = self.html_mut().and_then(|d| form_of(&d.dom, target));
                let navigate = match form {
                    Some(f) if submits => self.submit_form(f, Some(target)),
                    _ => None,
                };
                InputOutcome {
                    consumed: true,
                    navigate,
                }
            }
            None => {
                self.set_focus(None);
                InputOutcome::default()
            }
        }
    }

    /// A key press: `keydown` to scripts (at the focused element or the
    /// body), then editing of the focused text field.
    pub fn key_down(&mut self, key: &PageKey) -> InputOutcome {
        let Some(doc) = self.html_mut() else {
            return InputOutcome::default();
        };
        let focused = doc.dom.focused();
        let target = focused.or_else(|| doc.dom.find_element(doc.dom.root(), "body"));
        tracing::debug!(?key, ?focused, "page key");
        let init = EventInit {
            no_bubble: false,
            key: Some(key.dom_key()),
        };
        if let Some(t) = target
            && self.fire(t, "keydown", &init)
        {
            return InputOutcome {
                consumed: true,
                navigate: None,
            };
        }
        if let PageKey::Tab { shift } = key {
            return self.move_focus(*shift);
        }
        let Some(f) = focused else {
            return InputOutcome::default();
        };
        let kind = self.html_mut().and_then(|d| d.dom.control_kind(f));
        if !matches!(kind, Some(ControlKind::Text { .. })) {
            // Enter/space on a focused button activates it.
            if matches!(kind, Some(ControlKind::Button))
                && matches!(key, PageKey::Enter)
                && let Some(form) = self.html_mut().and_then(|d| form_of(&d.dom, f))
                && self.html_mut().is_some_and(|d| is_submit_button(&d.dom, f))
            {
                return InputOutcome {
                    consumed: true,
                    navigate: self.submit_form(form, Some(f)),
                };
            }
            return InputOutcome::default();
        }
        self.edit_text(f, key)
    }

    /// A key release: `keyup` to scripts.
    pub fn key_up(&mut self, key: &PageKey) {
        let Some(doc) = self.html_mut() else { return };
        let target = doc
            .dom
            .focused()
            .or_else(|| doc.dom.find_element(doc.dom.root(), "body"));
        if let Some(t) = target {
            self.fire(
                t,
                "keyup",
                &EventInit {
                    no_bubble: false,
                    key: Some(key.dom_key()),
                },
            );
        }
    }

    fn move_focus(&mut self, backwards: bool) -> InputOutcome {
        let Some(doc) = self.html_mut() else {
            return InputOutcome::default();
        };
        let order = focusable(&doc.dom);
        if order.is_empty() {
            return InputOutcome::default();
        }
        let current = doc
            .dom
            .focused()
            .and_then(|f| order.iter().position(|&n| n == f));
        let next = match (current, backwards) {
            (None, false) => 0,
            (None, true) => order.len() - 1,
            (Some(i), false) => (i + 1) % order.len(),
            (Some(i), true) => (i + order.len() - 1) % order.len(),
        };
        self.set_focus(Some(order[next]));
        InputOutcome {
            consumed: true,
            navigate: None,
        }
    }

    fn edit_text(&mut self, field: NodeId, key: &PageKey) -> InputOutcome {
        let consumed = InputOutcome {
            consumed: true,
            navigate: None,
        };
        let value = self.html_dom().control_value(field);
        let chars: Vec<char> = value.chars().collect();
        let caret = self.caret.min(chars.len());
        let (new_value, new_caret): (Option<String>, usize) = match key {
            PageKey::Text(t) => {
                let insert: String = t.chars().filter(|c| !c.is_control()).collect();
                if insert.is_empty() {
                    return consumed;
                }
                let mut v: String = chars[..caret].iter().collect();
                v.push_str(&insert);
                v.extend(&chars[caret..]);
                (Some(v), caret + insert.chars().count())
            }
            PageKey::Backspace if caret > 0 => {
                let mut c = chars.clone();
                c.remove(caret - 1);
                (Some(c.into_iter().collect()), caret - 1)
            }
            PageKey::Delete if caret < chars.len() => {
                let mut c = chars.clone();
                c.remove(caret);
                (Some(c.into_iter().collect()), caret)
            }
            PageKey::Left => (None, caret.saturating_sub(1)),
            PageKey::Right => (None, (caret + 1).min(chars.len())),
            PageKey::Home => (None, 0),
            PageKey::End => (None, chars.len()),
            PageKey::Enter => {
                // Implicit submission (HTML §4.10.21.2): Enter in a field
                // submits its form.
                let Some(form) = form_of(self.html_dom(), field) else {
                    return consumed;
                };
                if self.focus_value.as_deref() != Some(value.as_str()) {
                    self.focus_value = Some(value.clone());
                    self.fire(field, "change", &EventInit::default());
                }
                return InputOutcome {
                    consumed: true,
                    navigate: self.submit_form(form, None),
                };
            }
            PageKey::Backspace | PageKey::Delete => (None, caret),
            PageKey::Tab { .. } | PageKey::Escape | PageKey::Other(_) => {
                return InputOutcome::default();
            }
        };
        self.caret = new_caret;
        if let Some(v) = new_value {
            if let Some(doc) = self.html_mut() {
                doc.dom.set_control_value(field, &v);
            }
            self.form_changed();
            self.fire(field, "input", &EventInit::default());
        } else {
            // Only the caret moved.
            self.paint_dirty = true;
        }
        consumed
    }

    /// Submits `form` (HTML §4.10.21.3, GET only): fires `submit`, then
    /// returns the URL to navigate to. POST forms are not supported yet.
    fn submit_form(&mut self, form: NodeId, submitter: Option<NodeId>) -> Option<Url> {
        if self.fire(form, "submit", &EventInit::default()) {
            return None;
        }
        let doc = self.html_mut()?;
        let dom = &doc.dom;
        let method = dom
            .attribute(form, "method")
            .unwrap_or("get")
            .trim()
            .to_ascii_lowercase();
        if method != "get" {
            tracing::warn!(method, "form submission with this method is not supported");
            return None;
        }
        let base = doc.base_url.clone()?;
        let action = dom.attribute(form, "action").unwrap_or("").trim();
        let mut url = if action.is_empty() {
            base.clone()
        } else {
            base.join(action).ok()?
        };
        let mut pairs: Vec<(String, String)> = Vec::new();
        for n in dom.descendants(form) {
            let Some(kind) = dom.control_kind(n) else {
                continue;
            };
            let Some(name) = dom.attribute(n, "name").filter(|s| !s.is_empty()) else {
                continue;
            };
            if dom.attribute(n, "disabled").is_some() {
                continue;
            }
            let value = match kind {
                ControlKind::Text { .. } => dom.control_value(n),
                ControlKind::Checkbox | ControlKind::Radio if dom.checked(n) => {
                    dom.attribute(n, "value").unwrap_or("on").to_owned()
                }
                ControlKind::Button if Some(n) == submitter => {
                    dom.attribute(n, "value").unwrap_or("").to_owned()
                }
                _ => continue,
            };
            pairs.push((name.to_owned(), value));
        }
        // Hidden inputs are not rendered controls but are submitted.
        for n in dom.descendants(form) {
            if dom.is_element_named(n, "input")
                && dom
                    .attribute(n, "type")
                    .is_some_and(|t| t.trim().eq_ignore_ascii_case("hidden"))
                && let Some(name) = dom.attribute(n, "name").filter(|s| !s.is_empty())
            {
                pairs.push((
                    name.to_owned(),
                    dom.attribute(n, "value").unwrap_or("").to_owned(),
                ));
            }
        }
        url.set_fragment(None);
        url.query_pairs_mut().clear().extend_pairs(pairs);
        tracing::info!(%url, "form submitted");
        Some(url)
    }

    /// Paints the focus ring and, for text fields, the caret over the page.
    pub(crate) fn paint_focus(&mut self, list: &mut rustlite_paint::DisplayList) {
        let (scale, scroll) = (self.device_scale, self.scroll_y);
        let caret_chars = self.caret;
        let Content::Html(doc) = &mut self.content else {
            return;
        };
        let (Some(focused), Some(tree), Some(fonts)) =
            (doc.dom.focused(), doc.layout.as_ref(), self.fonts.as_mut())
        else {
            return;
        };
        let Some(g) = rustlite_layout::caret_geometry(tree, fonts, focused, caret_chars) else {
            return;
        };
        let device = |x: f32, y: f32, w: f32, h: f32| {
            rustlite_layout::RectF::new(x * scale, (y - scroll) * scale, w * scale, h * scale)
                .snapped()
        };
        let ring = (2.0 * scale).round().max(1.0) as u32;
        let b = g.border_box;
        list.stroke_rect(
            device(b.x - 2.0, b.y - 2.0, b.width + 4.0, b.height + 4.0),
            ring,
            FOCUS_RING,
        );
        if matches!(
            doc.dom.control_kind(focused),
            Some(ControlKind::Text { .. })
        ) {
            let r = device(g.x, g.top, 1.0, g.height);
            list.fill_rect(
                rustlite_paint::Rect::new(r.x, r.y, (scale.round() as u32).max(1), r.height),
                CARET,
            );
        }
    }
}

const FOCUS_RING: rustlite_paint::Color = rustlite_paint::Color::rgb(16, 16, 16);
const CARET: rustlite_paint::Color = rustlite_paint::Color::rgb(0, 0, 0);
