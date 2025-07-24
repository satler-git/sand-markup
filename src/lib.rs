pub mod formatter;
pub mod lsp;
pub mod parser;

// Re-export main functions for testing
pub use crate::main::{convert_parse_error, convert_pest_error, print_completions};
