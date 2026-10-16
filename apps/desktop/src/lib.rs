//! RustLite desktop shell.
//!
//! The browser UI (toolbar, window, shortcuts) lives here and is kept separate
//! from page state, which is owned by `rustlite-browser-core`.

pub mod address_bar;
pub mod app;
pub mod browser_window;
pub mod commands;
pub mod findbar;
pub mod frame;
pub mod options;
pub mod tabstrip;
pub mod toolbar;
