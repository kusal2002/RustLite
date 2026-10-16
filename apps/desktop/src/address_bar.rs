//! Address bar editing model (no rendering, no platform types beyond keys).

use winit::keyboard::{Key, ModifiersState, NamedKey};

/// What a key press did to the address bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// Text or caret changed; redraw.
    Changed,
    /// Enter: navigate to this input.
    Submit(String),
    /// Escape: editing abandoned (text reverted).
    Cancel,
    /// Copy this text to the clipboard (Ctrl/Cmd+C with a selection).
    Copy(String),
    /// Cut: this text was removed; put it on the clipboard.
    Cut(String),
    /// Paste: the caller reads the clipboard and calls [`AddressBar::insert`].
    Paste,
    /// Not an editing key; let other shortcuts handle it.
    Ignored,
}

/// Single-line text field with a caret and an all-or-nothing selection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AddressBar {
    text: String,
    /// Caret position as a byte index on a char boundary.
    cursor: usize,
    focused: bool,
    all_selected: bool,
    /// The committed URL, restored on Escape.
    committed: String,
}

impl AddressBar {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    pub fn is_all_selected(&self) -> bool {
        self.all_selected
    }

    /// Shows `url` unless the user is editing.
    pub fn show_url(&mut self, url: &str) {
        self.committed = url.to_owned();
        if !self.focused {
            self.text = url.to_owned();
            self.cursor = self.text.len();
        }
    }

    /// Focuses the field with everything selected (like clicking it or Ctrl+L).
    pub fn focus(&mut self) {
        self.focused = true;
        self.all_selected = !self.text.is_empty();
        self.cursor = self.text.len();
    }

    /// Unfocuses, reverting uncommitted edits.
    pub fn blur(&mut self) {
        self.focused = false;
        self.all_selected = false;
        self.text = self.committed.clone();
        self.cursor = self.text.len();
    }

    fn take_selection(&mut self) {
        if self.all_selected {
            self.text.clear();
            self.cursor = 0;
            self.all_selected = false;
        }
    }

    fn prev_boundary(&self) -> usize {
        self.text[..self.cursor]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    fn next_boundary(&self) -> usize {
        self.text[self.cursor..]
            .chars()
            .next()
            .map_or(self.cursor, |c| self.cursor + c.len_utf8())
    }

    /// Inserts typed text at the caret, replacing a selection. Control
    /// characters (including newlines from pastes) are dropped.
    pub fn insert(&mut self, s: &str) {
        let clean: String = s.chars().filter(|c| !c.is_control()).collect();
        if clean.is_empty() {
            return;
        }
        self.take_selection();
        self.text.insert_str(self.cursor, &clean);
        self.cursor += clean.len();
    }

    /// Handles a key press while focused.
    pub fn key(&mut self, key: &Key, text: Option<&str>, mods: ModifiersState) -> Edit {
        if !self.focused {
            return Edit::Ignored;
        }
        let primary = if cfg!(target_os = "macos") {
            mods.super_key()
        } else {
            mods.control_key()
        };
        match key {
            Key::Named(NamedKey::Enter) => {
                let input = self.text.trim().to_owned();
                self.focused = false;
                self.all_selected = false;
                Edit::Submit(input)
            }
            Key::Named(NamedKey::Escape) => {
                self.blur();
                Edit::Cancel
            }
            Key::Named(NamedKey::Backspace) => {
                if self.all_selected {
                    self.take_selection();
                } else if self.cursor > 0 {
                    let start = self.prev_boundary();
                    self.text.replace_range(start..self.cursor, "");
                    self.cursor = start;
                }
                Edit::Changed
            }
            Key::Named(NamedKey::Delete) => {
                if self.all_selected {
                    self.take_selection();
                } else if self.cursor < self.text.len() {
                    let end = self.next_boundary();
                    self.text.replace_range(self.cursor..end, "");
                }
                Edit::Changed
            }
            Key::Named(NamedKey::ArrowLeft) => {
                self.cursor = if self.all_selected {
                    0
                } else {
                    self.prev_boundary()
                };
                self.all_selected = false;
                Edit::Changed
            }
            Key::Named(NamedKey::ArrowRight) => {
                self.cursor = if self.all_selected {
                    self.text.len()
                } else {
                    self.next_boundary()
                };
                self.all_selected = false;
                Edit::Changed
            }
            Key::Named(NamedKey::Home) => {
                self.cursor = 0;
                self.all_selected = false;
                Edit::Changed
            }
            Key::Named(NamedKey::End) => {
                self.cursor = self.text.len();
                self.all_selected = false;
                Edit::Changed
            }
            Key::Character(c) if primary && c.eq_ignore_ascii_case("c") => {
                if self.all_selected {
                    Edit::Copy(self.text.clone())
                } else {
                    Edit::Ignored
                }
            }
            Key::Character(c) if primary && c.eq_ignore_ascii_case("x") => {
                if self.all_selected {
                    let text = std::mem::take(&mut self.text);
                    self.cursor = 0;
                    self.all_selected = false;
                    Edit::Cut(text)
                } else {
                    Edit::Ignored
                }
            }
            Key::Character(c) if primary && c.eq_ignore_ascii_case("v") => Edit::Paste,
            Key::Character(c) if primary && c.eq_ignore_ascii_case("a") => {
                self.all_selected = !self.text.is_empty();
                self.cursor = self.text.len();
                Edit::Changed
            }
            // Other shortcuts (Ctrl+Q, Cmd+L, Alt+Left, ...) pass through and
            // never type text. On macOS, Option+key types characters (e.g. "ø"),
            // so Alt alone is text input there.
            _ if mods.control_key()
                || mods.super_key()
                || (mods.alt_key() && !cfg!(target_os = "macos")) =>
            {
                Edit::Ignored
            }
            _ => match text {
                Some(t) if !t.is_empty() => {
                    self.insert(t);
                    Edit::Changed
                }
                _ => Edit::Ignored,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(k: NamedKey) -> Key {
        Key::Named(k)
    }

    fn type_text(bar: &mut AddressBar, s: &str) {
        for c in s.chars() {
            let t = c.to_string();
            bar.key(
                &Key::Character(t.as_str().into()),
                Some(&t),
                ModifiersState::empty(),
            );
        }
    }

    #[test]
    fn focus_selects_all_and_typing_replaces() {
        let mut bar = AddressBar::default();
        bar.show_url("https://old.example/");
        bar.focus();
        assert!(bar.is_all_selected());
        type_text(&mut bar, "new.test");
        assert_eq!(bar.text(), "new.test");
        assert_eq!(
            bar.key(&named(NamedKey::Enter), None, ModifiersState::empty()),
            Edit::Submit("new.test".into())
        );
        assert!(!bar.is_focused());
    }

    #[test]
    fn caret_editing_with_multibyte_text() {
        let mut bar = AddressBar::default();
        bar.focus();
        type_text(&mut bar, "aශb");
        let none = ModifiersState::empty();
        bar.key(&named(NamedKey::ArrowLeft), None, none);
        bar.key(&named(NamedKey::Backspace), None, none);
        assert_eq!(bar.text(), "ab");
        bar.key(&named(NamedKey::Home), None, none);
        bar.key(&named(NamedKey::Delete), None, none);
        assert_eq!(bar.text(), "b");
        assert_eq!(bar.cursor(), 0);
    }

    #[test]
    fn escape_reverts_and_url_updates_while_unfocused() {
        let mut bar = AddressBar::default();
        bar.show_url("https://a.test/");
        bar.focus();
        type_text(&mut bar, "typo");
        assert_eq!(
            bar.key(&named(NamedKey::Escape), None, ModifiersState::empty()),
            Edit::Cancel
        );
        assert_eq!(bar.text(), "https://a.test/");
        bar.show_url("https://b.test/");
        assert_eq!(bar.text(), "https://b.test/");
        bar.focus();
        type_text(&mut bar, "x");
        bar.show_url("https://c.test/");
        assert_eq!(
            bar.text(),
            "x",
            "a load must not clobber what the user is typing"
        );
    }

    #[test]
    fn shortcuts_pass_through_and_control_chars_are_dropped() {
        let mut bar = AddressBar::default();
        bar.focus();
        let q = Key::Character("q".into());
        // Both Ctrl and Cmd combinations are shortcuts on every platform.
        assert_eq!(
            bar.key(&q, Some("q"), ModifiersState::CONTROL),
            Edit::Ignored
        );
        assert_eq!(bar.key(&q, Some("q"), ModifiersState::SUPER), Edit::Ignored);
        assert_eq!(bar.text(), "", "shortcut keys must not type text");
        bar.insert("a\nb\tc");
        assert_eq!(bar.text(), "abc");
        assert_eq!(
            AddressBar::default().key(&q, Some("q"), ModifiersState::empty()),
            Edit::Ignored
        );
    }

    fn primary() -> ModifiersState {
        if cfg!(target_os = "macos") {
            ModifiersState::SUPER
        } else {
            ModifiersState::CONTROL
        }
    }

    #[test]
    fn copy_cut_and_paste() {
        let mut bar = AddressBar::default();
        bar.show_url("https://a.test/");
        bar.focus();
        let key = |c: &str| Key::Character(c.into());
        assert_eq!(
            bar.key(&key("c"), None, primary()),
            Edit::Copy("https://a.test/".into())
        );
        assert_eq!(
            bar.key(&key("x"), None, primary()),
            Edit::Cut("https://a.test/".into())
        );
        assert_eq!(bar.text(), "");
        // Without a selection there is nothing to copy or cut.
        bar.insert("abc");
        assert_eq!(bar.key(&key("c"), None, primary()), Edit::Ignored);
        assert_eq!(bar.key(&key("x"), None, primary()), Edit::Ignored);
        assert_eq!(bar.key(&key("v"), None, primary()), Edit::Paste);
        // Pasted text replaces a selection and loses line breaks.
        bar.key(&key("a"), None, primary());
        bar.insert("example.com\n");
        assert_eq!(bar.text(), "example.com");
    }
}
