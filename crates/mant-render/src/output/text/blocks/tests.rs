//! Output facts: physical rows, composed origins, term boundaries and styles.

use super::*;
use mant_ir::LayoutHint;

mod cell_boundaries;
mod container_geometry;
mod decorated_tables;
mod definition_relations;
mod definition_rows;
mod definition_tails;
mod fixtures;
mod navigation_markers;
mod owner_layout;
mod physical_rows;
mod table_gaps;
mod table_geometry;

use fixtures::{declared_column_table, navigation_table, paragraph, plain_list};
