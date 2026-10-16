//! Browser commands and their keyboard shortcuts.
//!
//! Only commands that are actually implemented are mapped. Navigation commands
//! (reload, back, forward, focus address bar) are added together with the
//! features they drive.

use winit::keyboard::{Key, ModifiersState, NamedKey};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Close the window and exit.
    Quit,
    /// Outline every painted rectangle (paint debugging aid).
    ToggleDebugOverlay,
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
        Key::Character(c) if primary_modifier(mods) && !extra => {
            match c.to_ascii_lowercase().as_str() {
                "q" | "w" => Some(Command::Quit),
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
}
