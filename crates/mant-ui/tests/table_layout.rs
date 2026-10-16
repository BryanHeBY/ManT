//! Table frames, responsive columns and topology through real consumers.

#[path = "support/table_matrix/mod.rs"]
mod table_matrix;

mod table_layout {
    mod geometry;
    mod native;
    mod navigation;
    mod real;
    mod rows;
}
