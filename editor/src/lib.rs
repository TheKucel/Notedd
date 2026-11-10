//! Notedd's editing kernel: a block tree with styled runs, no UI.
//! All structural changes go through `Core`; Markdown is only the file format.

pub mod core;
pub mod markdown;
pub mod model;

#[cfg(test)]
mod fuzz_tests;
#[cfg(test)]
mod nested_tests;
#[cfg(test)]
mod commonmark_tests;

pub use crate::core::{Core, EditKind};
pub use model::{Block, Doc, Kind, Marks, Pos, Run};
