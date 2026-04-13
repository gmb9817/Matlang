//! Token model for the Release 0.1 lexer.

use crate::source::SourceSpan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keyword {
    If,
    ElseIf,
    Else,
    End,
    For,
    While,
    Break,
    Continue,
    Return,
    Switch,
    Case,
    Otherwise,
    Try,
    Catch,
    Global,
    Persistent,
    Function,
    ClassDef,
    Properties,
    Methods,
}

impl Keyword {
    pub fn from_lexeme(lexeme: &str) -> Option<Self> {
        let keyword = match lexeme {
            "if" => Self::If,
            "elseif" => Self::ElseIf,
            "else" => Self::Else,
            "end" => Self::End,
            "for" => Self::For,
            "while" => Self::While,
            "break" => Self::Break,
            "continue" => Self::Continue,
            "return" => Self::Return,
            "switch" => Self::Switch,
            "case" => Self::Case,
            "otherwise" => Self::Otherwise,
            "try" => Self::Try,
            "catch" => Self::Catch,
            "global" => Self::Global,
            "persistent" => Self::Persistent,
            "function" => Self::Function,
            "classdef" => Self::ClassDef,
            "properties" => Self::Properties,
            "methods" => Self::Methods,
            _ => return None,
        };

        Some(keyword)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberLiteralKind {
    Integer,
    Float,
    Scientific,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorKind {
    Plus,
    Minus,
    Multiply,
    RightDivide,
    LeftDivide,
    Power,
    ElementwiseMultiply,
    ElementwiseRightDivide,
    ElementwiseLeftDivide,
    ElementwisePower,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
    Equal,
    NotEqual,
    LogicalAnd,
    LogicalOr,
    LogicalNot,
    ShortCircuitAnd,
    ShortCircuitOr,
    Assign,
    Colon,
    Dot,
    Transpose,
    DotTranspose,
    FunctionHandle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelimiterKind {
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    LeftBrace,
    RightBrace,
    Comma,
    Semicolon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriviaKind {
    Whitespace,
    Comment,
    LineContinuation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trivia {
    pub kind: TriviaKind,
    pub span: SourceSpan,
    pub lexeme: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Identifier,
    Keyword(Keyword),
    NumberLiteral(NumberLiteralKind),
    CharLiteral,
    StringLiteral,
    Operator(OperatorKind),
    Delimiter(DelimiterKind),
    Newline,
    EndOfFile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: SourceSpan,
    pub lexeme: String,
    pub leading_trivia: Vec<Trivia>,
    pub trailing_trivia: Vec<Trivia>,
}

impl Token {
    pub fn new(kind: TokenKind, span: SourceSpan, lexeme: impl Into<String>) -> Self {
        Self {
            kind,
            span,
            lexeme: lexeme.into(),
            leading_trivia: Vec::new(),
            trailing_trivia: Vec::new(),
        }
    }

    pub fn with_leading_trivia(mut self, leading_trivia: Vec<Trivia>) -> Self {
        self.leading_trivia = leading_trivia;
        self
    }

    pub fn with_trailing_trivia(mut self, trailing_trivia: Vec<Trivia>) -> Self {
        self.trailing_trivia = trailing_trivia;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DelimiterKind, Keyword, NumberLiteralKind, OperatorKind, Token, TokenKind, Trivia,
        TriviaKind,
    };
    use crate::source::{SourceFileId, SourcePosition, SourceSpan};

    fn span(start: u32, end: u32) -> SourceSpan {
        SourceSpan::new(
            SourceFileId(1),
            SourcePosition::new(start, 1, start + 1),
            SourcePosition::new(end, 1, end + 1),
        )
    }

    #[test]
    fn recognizes_release_0_1_keywords() {
        assert_eq!(Keyword::from_lexeme("if"), Some(Keyword::If));
        assert_eq!(Keyword::from_lexeme("function"), Some(Keyword::Function));
        assert_eq!(Keyword::from_lexeme("ClassDef"), None);
        assert_eq!(Keyword::from_lexeme("variable"), None);
    }

    #[test]
    fn token_preserves_trivia_and_lexeme() {
        let leading = vec![Trivia {
            kind: TriviaKind::Comment,
            span: span(0, 3),
            lexeme: "% a".to_string(),
        }];
        let trailing = vec![Trivia {
            kind: TriviaKind::Whitespace,
            span: span(4, 5),
            lexeme: " ".to_string(),
        }];

        let token = Token::new(
            TokenKind::NumberLiteral(NumberLiteralKind::Scientific),
            span(5, 10),
            "1e-3",
        )
        .with_leading_trivia(leading.clone())
        .with_trailing_trivia(trailing.clone());

        assert_eq!(token.lexeme, "1e-3");
        assert_eq!(token.leading_trivia, leading);
        assert_eq!(token.trailing_trivia, trailing);
    }

    #[test]
    fn token_kinds_cover_release_0_1_surface_categories() {
        let delimiter = TokenKind::Delimiter(DelimiterKind::LeftBracket);
        let operator = TokenKind::Operator(OperatorKind::ElementwisePower);

        assert!(matches!(delimiter, TokenKind::Delimiter(_)));
        assert!(matches!(operator, TokenKind::Operator(_)));
    }
}
