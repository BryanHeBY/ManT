//! Observe exact upstream renderer-golden differences for native `mantdoc`.
//!
//! This is an M9 evidence tool.  The small wrapper lets Cargo auto-discover the
//! development-only example without leaving an explicit target in the
//! published manifest.

#[cfg(feature = "render")]
#[path = "support/mantdoc-render-diff.rs"]
mod render_diff;

#[cfg(feature = "render")]
fn main() -> std::process::ExitCode {
    render_diff::main()
}

#[cfg(not(feature = "render"))]
fn main() -> std::process::ExitCode {
    eprintln!("mantdoc-render-diff requires the `render` feature");
    std::process::ExitCode::from(2)
}
