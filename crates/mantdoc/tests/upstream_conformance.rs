//! Native upstream-regression support and focused invariant tests.
//!
//! The slower archive-backed gates are deliberately driven by the private
//! examples in `../examples/`; ordinary unit checks stay archive-free.

#[path = "conformance/mod.rs"]
#[allow(dead_code, unused_imports)]
mod conformance;
