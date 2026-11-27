//! The app core shared by the desktop (Windows, Linux) and phone (Android) shells (ADR-015).
//!
//! No Tauri here: each shell wires these into its own commands and events, so a change to how a
//! session runs, which settings apply, or what the window receives is made once for both apps.
//! Platform pieces (the capture backend, call context, file locations) come in from the shell.

pub mod clock;
pub mod models;
pub mod payload;
pub mod session;
pub mod settings;
