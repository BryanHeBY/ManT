//! Output facts: physical rows, composed origins, term boundaries and styles.

use super::*;
use mant_ir::LayoutHint;

mod container_geometry;
mod decorated_tables;
mod definition_rows;
mod fixtures;
mod physical_rows;
mod table_geometry;

use fixtures::{declared_column_table, navigation_table, paragraph, plain_list};
