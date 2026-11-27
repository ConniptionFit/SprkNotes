//! The app core shared by the desktop (Windows, Linux) and phone (Android) shells (ADR-015).
//!
//! No Tauri here: each shell wires these into its own commands and events, so a change to how a
//! session runs, which settings apply, or what the window receives is made once for both apps.
//! Platform pieces (the capture backend, call context, file locations) come in from the shell.

pub mod clock;
pub mod download;
pub mod models;
/// The https client for model downloads (WinHTTP stays in the Windows shell).
#[cfg(all(feature = "https", not(windows)))]
pub mod net;
pub mod payload;
pub mod session;
pub mod settings;
