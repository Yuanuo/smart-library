//! smart-library-core: core library of Smart Library, a local knowledge base.
//!
//! M0a: read a CBETA `bookcase.zip` (or an unzipped directory), assemble the
//! catalog / navigation trees / per-book table of contents, without touching
//! the volume XML bodies yet.

pub mod bookcase;
pub mod catalog;
pub mod markup;
pub mod nav;
pub mod source;
pub mod text;
pub mod toc;
pub mod volume;
