//! Snot's storage and document core.
//!
//! Deliberately free of any UI or platform dependency: the same crate backs
//! the desktop and mobile shells, and can be linked into a CLI or a sync
//! daemon without dragging a webview along.

pub mod error;
pub mod export;
pub mod import;
pub mod model;
pub mod seed;
pub mod store;

pub use error::{Error, Result};
pub use export::doc_to_markdown;
pub use import::{
    import_markdown_dir, import_samsung_dir, markdown_to_doc, ImportFailure, ImportSummary,
};
pub use model::{
    derive_preview, derive_title, doc_to_text, now_ms, Attachment, Folder, Millis, Note, NotePatch,
    NoteSummary, Scope, SortBy, Tag,
};
pub use store::{empty_doc, Store};
