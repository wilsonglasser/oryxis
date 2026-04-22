//! Per-screen view methods for `Oryxis`, split out of `app.rs`.
//!
//! Each submodule defines an additional `impl Oryxis` block with the view
//! functions for a specific screen or panel. Call sites in `app.rs` look
//! unchanged: `self.view_dashboard()`, `self.view_settings()`, etc. — Rust
//! allows `impl` blocks for the same type to be scattered across files.

pub(crate) mod chrome;
pub(crate) mod connection_progress;
pub(crate) mod dashboard;
pub(crate) mod history;
pub(crate) mod host_panel;
pub(crate) mod icon_picker;
pub(crate) mod keys;
pub(crate) mod known_hosts;
pub(crate) mod layout;
pub(crate) mod new_tab_picker;
pub(crate) mod settings;
pub(crate) mod sidebar;
pub(crate) mod snippets;
pub(crate) mod status_bar;
pub(crate) mod tab_bar;
pub(crate) mod terminal;
pub(crate) mod update_modal;
pub(crate) mod vault;
