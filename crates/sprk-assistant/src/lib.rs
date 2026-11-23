//! The assistant core (ADR-009): everything that can be tested without a real model.
//!
//! - [`engine`]: the `AssistantEngine` trait, run control, and a scripted fake for tests.
//! - [`view`]: turns committed transcript lines into a compact, line-numbered prompt view.
//! - [`prompt`]: fixed prompt formats owned by the app (a pack only names one, never ships one).
//! - [`title`]: the dynamic note title feature, with cleaning and a grounding check.
//! - [`governor`]: tier choice from free memory, yielding to transcription, idle unload, threads.
//! - [`protocol`]: the JSON lines spoken between the app and the helper process.
//!
//! Privacy: prompts, answers and titles are PHI-grade. Nothing in this crate logs them, and no
//! error value carries transcript text.
pub mod engine;
pub mod governor;
pub mod prompt;
pub mod protocol;
pub mod title;
pub mod view;

pub use engine::{AssistantEngine, Control, EngineError, GenRequest, GenStats, RunControl};
pub use governor::Tier;
