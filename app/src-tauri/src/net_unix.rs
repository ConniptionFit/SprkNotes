//! HTTPS client for model downloads and updates off Windows: the shared one in `sprk-core`.
#![cfg(not(windows))]

pub use sprk_core::net::Https;
