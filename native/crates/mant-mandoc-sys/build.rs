//! Placeholder build hook required by Cargo's `links` contract.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
}
