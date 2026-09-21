//! Experimental typed facade for native structured rendering.
//!
//! This module is deliberately feature-gated and hidden from generated
//! documentation while the structured result model is integrated by its first
//! consumer.  In particular, none of the numeric C ABI discriminants cross
//! this boundary.
#![allow(missing_docs)]

use std::num::NonZeroU32;

mod content;
mod document;
mod options;
mod source;

pub use content::*;
pub use document::*;
pub use options::*;
pub use source::*;

macro_rules! key_type {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(NonZeroU32);

        impl $name {
            #[must_use]
            pub const fn new(value: u32) -> Option<Self> {
                match NonZeroU32::new(value) {
                    Some(value) => Some(Self(value)),
                    None => None,
                }
            }

            #[must_use]
            pub const fn get(self) -> u32 {
                self.0.get()
            }
        }
    };
}

key_type!(SourceKey);
key_type!(SpanKey);
key_type!(ProvenanceKey);
key_type!(OwnerKey);
key_type!(ContentRootKey);
key_type!(ContentAtomKey);
key_type!(LinkOccurrenceKey);
key_type!(NativeBlockKey);

#[cfg(test)]
mod tests;
