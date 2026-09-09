#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

pub mod cells;
mod output;
mod presentation;

pub use output::*;
pub use presentation::*;
