//! Model packs: manifest, verify, import, resolve, rollback (SPEC 16).
//! Models are data, not code: no model path or name is hard-coded anywhere.
pub mod import;
pub mod manifest;
pub mod pack;
pub mod resolver;
pub mod store;

pub use manifest::{Manifest, ManifestError};
pub use store::{id_version, ActiveFile, Installed, ModelStore, StoreError, StoreKind};
