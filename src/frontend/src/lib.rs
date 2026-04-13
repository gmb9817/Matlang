//! Front-end crate for lexing, parsing, AST construction, and syntax diagnostics.

pub mod ast;
pub mod diagnostics;
pub mod lexer;
pub mod parser;
pub mod source;
pub mod testing;

pub const CRATE_NAME: &str = "matlab-frontend";

pub fn summary() -> &'static str {
    "Owns lexing, parsing, AST construction, source spans, and syntax diagnostics."
}
