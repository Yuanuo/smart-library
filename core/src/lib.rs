//! smart-library-core: core library of Smart Library, a local knowledge base.
//!
//! * M0a: read a CBETA `bookcase.zip` (or an unzipped directory), assemble the
//!   catalog / navigation trees / per-book table of contents (`bookcase`, ...)
//! * M0b: reading text of a volume XML with line map and anchors (`volume`)
//! * M0c: `aio` analyzer, Tantivy index and search, hit location (`analyzer`, `search`)

pub mod analyzer;
pub mod bookcase;
pub mod bookinfo;
pub mod catalog;
pub mod markup;
pub mod nav;
pub mod render;
pub mod search;
pub mod source;
pub mod text;
pub mod toc;
pub mod volume;
