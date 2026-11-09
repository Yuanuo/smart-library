//! smart-library-core: core library of Smart Library, a local knowledge base.
//!
//! * M0a: read a CBETA `bookcase.zip` (or an unzipped directory), assemble the
//!   catalog / navigation trees / per-book table of contents (`bookcase`, ...)
//! * M0b: reading text of a volume XML with line map and anchors (`volume`)
//! * M0c: `aio` analyzer, Tantivy index and search, hit location (`analyzer`, `search`)
//! * M2: title quick search (`titles`), CBETA citations (`cite`)

pub mod analyzer;
pub mod article;
pub mod attach;
pub mod bookcase;
pub mod bookinfo;
pub mod catalog;
pub mod cite;
pub mod exchange;
pub mod folder;
pub mod history;
pub mod library;
pub mod mindmap;
pub mod whiteboard;
pub mod diary;
pub mod marks;
pub mod markdown;
pub mod markup;
pub mod nav;
pub mod notes;
pub mod props;
pub mod render;
pub mod search;
pub mod sidecar;
pub mod source;
pub mod text;
pub mod titles;
pub mod toc;
pub mod volume;
