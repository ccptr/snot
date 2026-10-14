//! Snot's core: the document model, ink geometry, editing, storage and
//! search. No UI and no platform code — the desktop and Android front-ends
//! both sit on top of this crate, and everything they share lives here.

pub mod color;
pub mod doc;
pub mod editor;
pub mod error;
pub mod ink;
pub mod markdown;
pub mod note;
pub mod page;
pub mod store;
mod welcome;

pub use doc::{Block, Content, Doc, ImageBlock, Mark, ParaStyle, Run, Span, TextBlock};
pub use editor::{Caret, EditSession, Insert};
pub use error::{Error, Result};
pub use ink::{InkLayer, Paper, Point, Stroke, Tool};
pub use note::{Counts, Folder, Millis, Note, NoteSummary, Scope, SortBy, now_ms};
pub use store::{Attachment, Library};
