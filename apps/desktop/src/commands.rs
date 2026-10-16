//! Browser commands and their keyboard shortcuts.
//!
//! Only commands that are actually implemented are mapped.

use winit::keyboard::{Key, ModifiersState, NamedKey};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Close the window and exit.
    Quit,
    /// Outline every painted rectangle (paint debugging aid).
    ToggleDebugOverlay,
    /// Scroll by a number of lines (positive = down).
    ScrollLines(i32),
    /// Scroll by a number of viewport pages (positive = down).
    ScrollPages(i32),
    ScrollToTop,
    ScrollToBottom,
    /// Focus the address bar and select its text.
    FocusAddress,
    Back,
    Forward,
    Reload,
    /// Stop the pending navigation.
    Stop,
}

/// The platform's primary shortcut modifier: Cmd on macOS, Ctrl elsewhere.
fn primary_modifier(mods: ModifiersState) -> bool {
    if cfg!(target_os = "macos") {
        mods.super_key()
    } else {
        mods.control_key()
    }
}

/// Maps a key press to a command, if it is a known shortcut.
pub fn shortcut(key: &Key, mods: ModifiersState) -> Option<Command> {
    let extra = mods.shift_key() || mods.alt_key();
    match key {
        Key::Named(NamedKey::F12) if mods.is_empty() => Some(Command::ToggleDebugOverlay),
        Key::Named(NamedKey::ArrowDown) if mods.is_empty() => Some(Command::ScrollLines(1)),
        Key::Named(NamedKey::ArrowUp) if mods.is_empty() => Some(Command::ScrollLines(-1)),
        Key::Named(NamedKey::PageDown) => Some(Command::ScrollPages(1)),
        Key::Named(NamedKey::PageUp) => Some(Command::ScrollPages(-1)),
        Key::Named(NamedKey::Space) if mods.is_empty() => Some(Command::ScrollPages(1)),
        Key::Named(NamedKey::Space) if mods == ModifiersState::SHIFT => {
            Some(Command::ScrollPages(-1))
        }
        Key::Named(NamedKey::Home) => Some(Command::ScrollToTop),
        Key::Named(NamedKey::End) => Some(Command::ScrollToBottom),
        Key::Named(NamedKey::F5) if mods.is_empty() => Some(Command::Reload),
        Key::Named(NamedKey::F6) if mods.is_empty() => Some(Command::FocusAddress),
        Key::Named(NamedKey::Escape) if mods.is_empty() => Some(Command::Stop),
        Key::Named(NamedKey::ArrowLeft) if mods == ModifiersState::ALT => Some(Command::Back),
        Key::Named(NamedKey::ArrowRight) if mods == ModifiersState::ALT => Some(Command::Forward),
        Key::Character(c) if mods == ModifiersState::ALT && c.eq_ignore_ascii_case("d") => {
            Some(Command::FocusAddress)
        }
        Key::Character(c) if primary_modifier(mods) && !extra => {
            match c.to_ascii_lowercase().as_str() {
                "q" | "w" => Some(Command::Quit),
                "l" => Some(Command::FocusAddress),
                "r" => Some(Command::Reload),
                "[" if cfg!(target_os = "macos") => Some(Command::Back),
                "]" if cfg!(target_os = "macos") => Some(Command::Forward),
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn primary() -> ModifiersState {
        if cfg!(target_os = "macos") {
            ModifiersState::SUPER
        } else {
            ModifiersState::CONTROL
        }
    }

    #[test]
    fn quit_shortcuts() {
        assert_eq!(
            shortcut(&Key::Character("q".into()), primary()),
            Some(Command::Quit)
        );
        assert_eq!(
            shortcut(&Key::Character("W".into()), primary()),
            Some(Command::Quit)
        );
    }

    #[test]
    fn plain_letters_are_not_shortcuts() {
        assert_eq!(
            shortcut(&Key::Character("q".into()), ModifiersState::empty()),
            None
        );
        assert_eq!(
            shortcut(
                &Key::Character("q".into()),
                primary() | ModifiersState::SHIFT
            ),
            None
        );
    }

    #[test]
    fn f12_toggles_overlay() {
        assert_eq!(
            shortcut(&Key::Named(NamedKey::F12), ModifiersState::empty()),
            Some(Command::ToggleDebugOverlay)
        );
    }

    #[test]
    fn scrolling_keys() {
        let none = ModifiersState::empty();
        assert_eq!(
            shortcut(&Key::Named(NamedKey::ArrowDown), none),
            Some(Command::ScrollLines(1))
        );
        assert_eq!(
            shortcut(&Key::Named(NamedKey::PageUp), none),
            Some(Command::ScrollPages(-1))
        );
        assert_eq!(
            shortcut(&Key::Named(NamedKey::Space), ModifiersState::SHIFT),
            Some(Command::ScrollPages(-1))
        );
        assert_eq!(
            shortcut(&Key::Named(NamedKey::End), none),
            Some(Command::ScrollToBottom)
        );
        assert_eq!(
            shortcut(&Key::Named(NamedKey::ArrowDown), ModifiersState::CONTROL),
            None
        );
    }

    #[test]
    fn navigation_shortcuts() {
        let none = ModifiersState::empty();
        assert_eq!(
            shortcut(&Key::Named(NamedKey::F5), none),
            Some(Command::Reload)
        );
        assert_eq!(
            shortcut(&Key::Character("r".into()), primary()),
            Some(Command::Reload)
        );
        assert_eq!(
            shortcut(&Key::Character("l".into()), primary()),
            Some(Command::FocusAddress)
        );
        assert_eq!(
            shortcut(&Key::Named(NamedKey::ArrowLeft), ModifiersState::ALT),
            Some(Command::Back)
        );
        assert_eq!(
            shortcut(&Key::Named(NamedKey::ArrowRight), ModifiersState::ALT),
            Some(Command::Forward)
        );
        assert_eq!(
            shortcut(&Key::Named(NamedKey::Escape), none),
            Some(Command::Stop)
        );
        // Plain arrows still scroll.
        assert_eq!(shortcut(&Key::Named(NamedKey::ArrowLeft), none), None);
    }
}
