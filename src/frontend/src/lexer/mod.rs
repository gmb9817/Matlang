//! Lexer-facing token and trivia definitions.

mod scanner;
pub mod token;

pub use scanner::{lex, LexerOutput};
pub use token::{
    DelimiterKind, Keyword, NumberLiteralKind, OperatorKind, Token, TokenKind, Trivia, TriviaKind,
};
