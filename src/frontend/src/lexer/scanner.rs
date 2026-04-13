//! Release 0.1 lexer scaffold.

use crate::{
    diagnostics::Diagnostic,
    lexer::token::{
        DelimiterKind, Keyword, NumberLiteralKind, OperatorKind, Token, TokenKind, Trivia,
        TriviaKind,
    },
    source::{SourceFileId, SourcePosition, SourceSpan},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexerOutput {
    pub tokens: Vec<Token>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn lex(source: &str, file_id: SourceFileId) -> LexerOutput {
    let mut lexer = Lexer::new(source, file_id);
    lexer.lex_all();
    lexer.finish()
}

struct Lexer<'a> {
    source: &'a str,
    file_id: SourceFileId,
    cursor: usize,
    line: u32,
    column: u32,
    pending_trivia: Vec<Trivia>,
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str, file_id: SourceFileId) -> Self {
        Self {
            source,
            file_id,
            cursor: 0,
            line: 1,
            column: 1,
            pending_trivia: Vec::new(),
            tokens: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn finish(mut self) -> LexerOutput {
        let eof_span = self.zero_width_span();
        let eof =
            Token::new(TokenKind::EndOfFile, eof_span, "").with_leading_trivia(self.pending_trivia);
        self.tokens.push(eof);

        LexerOutput {
            tokens: self.tokens,
            diagnostics: self.diagnostics,
        }
    }

    fn lex_all(&mut self) {
        while let Some(ch) = self.peek_char() {
            match ch {
                ' ' | '\t' => self.lex_whitespace(),
                '\r' | '\n' => self.lex_newline(),
                '%' => self.lex_comment(),
                '.' if self.is_line_continuation() => self.lex_line_continuation(),
                'A'..='Z' | 'a'..='z' | '_' => self.lex_identifier_or_keyword(),
                '0'..='9' => self.lex_number(),
                '\'' => {
                    if self.should_lex_transpose() {
                        self.lex_transpose_operator();
                    } else {
                        self.lex_char_literal();
                    }
                }
                '"' => self.lex_string_literal(),
                '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' => self.lex_delimiter(),
                '+' | '-' | '*' | '/' | '\\' | '^' | '<' | '>' | '=' | '~' | '&' | '|' | ':'
                | '@' => self.lex_operator(),
                '.' => {
                    if self.try_lex_dot_operator() {
                        continue;
                    }
                    self.lex_dot();
                }
                _ => self.lex_unknown(),
            }
        }
    }

    fn lex_whitespace(&mut self) {
        let start = self.position();
        let mut lexeme = String::new();

        while let Some(ch) = self.peek_char() {
            if ch == ' ' || ch == '\t' {
                lexeme.push(ch);
                self.bump_char();
            } else {
                break;
            }
        }

        self.pending_trivia.push(Trivia {
            kind: TriviaKind::Whitespace,
            span: self.span_from(start),
            lexeme,
        });
    }

    fn lex_comment(&mut self) {
        let start = self.position();
        let mut lexeme = String::new();

        while let Some(ch) = self.peek_char() {
            if ch == '\r' || ch == '\n' {
                break;
            }

            lexeme.push(ch);
            self.bump_char();
        }

        self.pending_trivia.push(Trivia {
            kind: TriviaKind::Comment,
            span: self.span_from(start),
            lexeme,
        });
    }

    fn lex_line_continuation(&mut self) {
        let start = self.position();
        let mut lexeme = String::new();

        for _ in 0..3 {
            lexeme.push('.');
            self.bump_char();
        }

        while let Some(ch) = self.peek_char() {
            if ch == '\r' || ch == '\n' {
                break;
            }

            lexeme.push(ch);
            self.bump_char();
        }

        if self.peek_char() == Some('\r') {
            lexeme.push('\r');
            self.bump_char();
        }

        if self.peek_char() == Some('\n') {
            lexeme.push('\n');
            self.bump_char();
        }

        self.pending_trivia.push(Trivia {
            kind: TriviaKind::LineContinuation,
            span: self.span_from(start),
            lexeme,
        });
    }

    fn lex_newline(&mut self) {
        let start = self.position();
        let mut lexeme = String::new();

        if self.peek_char() == Some('\r') {
            lexeme.push('\r');
            self.bump_char();
        }

        if self.peek_char() == Some('\n') {
            lexeme.push('\n');
            self.bump_char();
        }

        let token = Token::new(TokenKind::Newline, self.span_from(start), lexeme)
            .with_leading_trivia(std::mem::take(&mut self.pending_trivia));
        self.tokens.push(token);
    }

    fn lex_identifier_or_keyword(&mut self) {
        let start = self.position();
        let mut lexeme = String::new();

        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                lexeme.push(ch);
                self.bump_char();
            } else {
                break;
            }
        }

        let kind = Keyword::from_lexeme(&lexeme)
            .map(TokenKind::Keyword)
            .unwrap_or(TokenKind::Identifier);
        self.push_token(kind, start, lexeme);
    }

    fn lex_number(&mut self) {
        let start = self.position();
        let mut lexeme = String::new();
        let mut seen_dot = false;
        let mut kind = NumberLiteralKind::Integer;

        while let Some(ch) = self.peek_char() {
            match ch {
                '0'..='9' => {
                    lexeme.push(ch);
                    self.bump_char();
                }
                '.' if !seen_dot => {
                    if matches!(
                        self.peek_next_char(),
                        Some('.') | Some('*' | '/' | '\\' | '^' | '\'')
                    ) {
                        break;
                    }

                    seen_dot = true;
                    kind = NumberLiteralKind::Float;
                    lexeme.push(ch);
                    self.bump_char();
                }
                'e' | 'E' => {
                    kind = NumberLiteralKind::Scientific;
                    lexeme.push(ch);
                    self.bump_char();
                    if let Some(sign @ ('+' | '-')) = self.peek_char() {
                        lexeme.push(sign);
                        self.bump_char();
                    }
                }
                _ => break,
            }
        }

        if matches!(self.peek_char(), Some('i' | 'j'))
            && !self
                .peek_nth_char(1)
                .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            lexeme.push(self.bump_char().expect("imaginary suffix expected"));
        }

        self.push_token(TokenKind::NumberLiteral(kind), start, lexeme);
    }

    fn lex_char_literal(&mut self) {
        self.lex_quoted_literal(
            '\'',
            TokenKind::CharLiteral,
            "LEX001",
            "unterminated char literal",
        );
    }

    fn lex_string_literal(&mut self) {
        self.lex_quoted_literal(
            '"',
            TokenKind::StringLiteral,
            "LEX002",
            "unterminated string literal",
        );
    }

    fn lex_quoted_literal(
        &mut self,
        delimiter: char,
        kind: TokenKind,
        error_code: &'static str,
        error_message: &'static str,
    ) {
        let start = self.position();
        let mut lexeme = String::new();
        lexeme.push(delimiter);
        self.bump_char();

        let mut terminated = false;

        while let Some(ch) = self.peek_char() {
            lexeme.push(ch);
            self.bump_char();

            if ch == delimiter {
                if self.peek_char() == Some(delimiter) {
                    lexeme.push(delimiter);
                    self.bump_char();
                    continue;
                }

                terminated = true;
                break;
            }

            if ch == '\r' || ch == '\n' {
                break;
            }
        }

        let span = self.span_from(start);
        if terminated {
            let token = Token::new(kind, span, lexeme)
                .with_leading_trivia(std::mem::take(&mut self.pending_trivia));
            self.tokens.push(token);
        } else {
            self.diagnostics
                .push(Diagnostic::error(error_code, error_message, span));
        }
    }

    fn lex_delimiter(&mut self) {
        let start = self.position();
        let ch = self.bump_char().expect("delimiter expected");
        let kind = match ch {
            '(' => DelimiterKind::LeftParen,
            ')' => DelimiterKind::RightParen,
            '[' => DelimiterKind::LeftBracket,
            ']' => DelimiterKind::RightBracket,
            '{' => DelimiterKind::LeftBrace,
            '}' => DelimiterKind::RightBrace,
            ',' => DelimiterKind::Comma,
            ';' => DelimiterKind::Semicolon,
            _ => unreachable!("only delimiter characters reach this branch"),
        };
        self.push_token(TokenKind::Delimiter(kind), start, ch.to_string());
    }

    fn lex_operator(&mut self) {
        let start = self.position();
        let first = self.bump_char().expect("operator expected");
        let mut lexeme = first.to_string();

        let kind = match first {
            '+' => OperatorKind::Plus,
            '-' => OperatorKind::Minus,
            '*' => OperatorKind::Multiply,
            '/' => OperatorKind::RightDivide,
            '\\' => OperatorKind::LeftDivide,
            '^' => OperatorKind::Power,
            '<' => {
                if self.peek_char() == Some('=') {
                    lexeme.push('=');
                    self.bump_char();
                    OperatorKind::LessThanOrEqual
                } else {
                    OperatorKind::LessThan
                }
            }
            '>' => {
                if self.peek_char() == Some('=') {
                    lexeme.push('=');
                    self.bump_char();
                    OperatorKind::GreaterThanOrEqual
                } else {
                    OperatorKind::GreaterThan
                }
            }
            '=' => {
                if self.peek_char() == Some('=') {
                    lexeme.push('=');
                    self.bump_char();
                    OperatorKind::Equal
                } else {
                    OperatorKind::Assign
                }
            }
            '~' => {
                if self.peek_char() == Some('=') {
                    lexeme.push('=');
                    self.bump_char();
                    OperatorKind::NotEqual
                } else {
                    OperatorKind::LogicalNot
                }
            }
            '&' => {
                if self.peek_char() == Some('&') {
                    lexeme.push('&');
                    self.bump_char();
                    OperatorKind::ShortCircuitAnd
                } else {
                    OperatorKind::LogicalAnd
                }
            }
            '|' => {
                if self.peek_char() == Some('|') {
                    lexeme.push('|');
                    self.bump_char();
                    OperatorKind::ShortCircuitOr
                } else {
                    OperatorKind::LogicalOr
                }
            }
            ':' => OperatorKind::Colon,
            '@' => OperatorKind::FunctionHandle,
            _ => unreachable!("only operator characters reach this branch"),
        };

        self.push_token(TokenKind::Operator(kind), start, lexeme);
    }

    fn should_lex_transpose(&self) -> bool {
        if self
            .pending_trivia
            .iter()
            .any(|trivia| trivia.kind != TriviaKind::LineContinuation)
            && self.apostrophe_starts_quoted_text()
        {
            return false;
        }
        matches!(
            self.tokens.last().map(|token| &token.kind),
            Some(TokenKind::Identifier)
                | Some(TokenKind::NumberLiteral(_))
                | Some(TokenKind::CharLiteral)
                | Some(TokenKind::StringLiteral)
                | Some(TokenKind::Delimiter(DelimiterKind::RightParen))
                | Some(TokenKind::Delimiter(DelimiterKind::RightBracket))
                | Some(TokenKind::Delimiter(DelimiterKind::RightBrace))
                | Some(TokenKind::Operator(OperatorKind::Transpose))
                | Some(TokenKind::Operator(OperatorKind::DotTranspose))
        )
    }

    fn apostrophe_starts_quoted_text(&self) -> bool {
        let mut offset = 1usize;
        while let Some(ch) = self.peek_nth_char(offset) {
            if ch == '\r' || ch == '\n' {
                return false;
            }
            if ch == '\'' {
                if self.peek_nth_char(offset + 1) == Some('\'') {
                    offset += 2;
                    continue;
                }
                return true;
            }
            offset += 1;
        }
        false
    }

    fn lex_transpose_operator(&mut self) {
        let start = self.position();
        self.bump_char();
        self.push_token(TokenKind::Operator(OperatorKind::Transpose), start, "'");
    }

    fn try_lex_dot_operator(&mut self) -> bool {
        let start = self.position();
        let Some(next) = self.peek_next_char() else {
            return false;
        };

        let kind = match next {
            '*' => OperatorKind::ElementwiseMultiply,
            '/' => OperatorKind::ElementwiseRightDivide,
            '\\' => OperatorKind::ElementwiseLeftDivide,
            '^' => OperatorKind::ElementwisePower,
            '\'' => OperatorKind::DotTranspose,
            _ => return false,
        };

        self.bump_char();
        self.bump_char();
        let lexeme = match kind {
            OperatorKind::ElementwiseMultiply => ".*",
            OperatorKind::ElementwiseRightDivide => "./",
            OperatorKind::ElementwiseLeftDivide => ".\\",
            OperatorKind::ElementwisePower => ".^",
            OperatorKind::DotTranspose => ".'",
            _ => unreachable!(),
        };
        self.push_token(TokenKind::Operator(kind), start, lexeme);
        true
    }

    fn lex_dot(&mut self) {
        let start = self.position();
        self.bump_char();
        self.push_token(TokenKind::Operator(OperatorKind::Dot), start, ".");
    }

    fn lex_unknown(&mut self) {
        let start = self.position();
        let ch = self.bump_char().expect("unknown character expected");
        let span = self.span_from(start);
        self.diagnostics.push(Diagnostic::error(
            "LEX999",
            format!("unsupported character `{ch}`"),
            span,
        ));
    }

    fn push_token(&mut self, kind: TokenKind, start: SourcePosition, lexeme: impl Into<String>) {
        let token = Token::new(kind, self.span_from(start), lexeme)
            .with_leading_trivia(std::mem::take(&mut self.pending_trivia));
        self.tokens.push(token);
    }

    fn is_line_continuation(&self) -> bool {
        self.peek_char() == Some('.')
            && self.peek_next_char() == Some('.')
            && self.peek_nth_char(2) == Some('.')
    }

    fn zero_width_span(&self) -> SourceSpan {
        let pos = self.position();
        SourceSpan::new(self.file_id, pos, pos)
    }

    fn span_from(&self, start: SourcePosition) -> SourceSpan {
        SourceSpan::new(self.file_id, start, self.position())
    }

    fn position(&self) -> SourcePosition {
        SourcePosition::new(self.cursor as u32, self.line, self.column)
    }

    fn peek_char(&self) -> Option<char> {
        self.source[self.cursor..].chars().next()
    }

    fn peek_next_char(&self) -> Option<char> {
        self.peek_nth_char(1)
    }

    fn peek_nth_char(&self, n: usize) -> Option<char> {
        self.source[self.cursor..].chars().nth(n)
    }

    fn bump_char(&mut self) -> Option<char> {
        let ch = self.peek_char()?;
        self.cursor += ch.len_utf8();

        match ch {
            '\n' => {
                self.line += 1;
                self.column = 1;
            }
            _ => {
                self.column += 1;
            }
        }

        Some(ch)
    }
}

#[cfg(test)]
mod tests {
    use super::lex;
    use crate::{
        lexer::{Keyword, NumberLiteralKind, OperatorKind, TokenKind, TriviaKind},
        source::SourceFileId,
    };

    #[test]
    fn lexes_identifiers_keywords_numbers_and_delimiters() {
        let output = lex("function y = foo(x)\n", SourceFileId(1));

        assert!(output.diagnostics.is_empty());
        assert_eq!(output.tokens[0].kind, TokenKind::Keyword(Keyword::Function));
        assert_eq!(output.tokens[1].kind, TokenKind::Identifier);
        assert_eq!(
            output.tokens[2].kind,
            TokenKind::Operator(OperatorKind::Assign)
        );
        assert_eq!(
            output.tokens[4].kind,
            TokenKind::Delimiter(crate::lexer::DelimiterKind::LeftParen)
        );
        assert_eq!(
            output.tokens[6].kind,
            TokenKind::Delimiter(crate::lexer::DelimiterKind::RightParen)
        );
        assert_eq!(output.tokens[7].kind, TokenKind::Newline);
        assert_eq!(output.tokens[8].kind, TokenKind::EndOfFile);
    }

    #[test]
    fn lexes_scientific_numbers_and_dot_operators() {
        let output = lex("x = 1e-3 .* y.^2", SourceFileId(1));

        assert!(output.diagnostics.is_empty());
        assert_eq!(
            output.tokens[2].kind,
            TokenKind::NumberLiteral(NumberLiteralKind::Scientific)
        );
        assert_eq!(
            output.tokens[3].kind,
            TokenKind::Operator(OperatorKind::ElementwiseMultiply)
        );
        assert_eq!(
            output.tokens[5].kind,
            TokenKind::Operator(OperatorKind::ElementwisePower)
        );
    }

    #[test]
    fn lexes_imaginary_suffix_numeric_literals() {
        let output = lex("x = 1i + 2.5j + 3e-2i", SourceFileId(1));

        assert!(output.diagnostics.is_empty());
        assert_eq!(
            output.tokens[2].kind,
            TokenKind::NumberLiteral(NumberLiteralKind::Integer)
        );
        assert_eq!(output.tokens[2].lexeme, "1i");
        assert_eq!(
            output.tokens[4].kind,
            TokenKind::NumberLiteral(NumberLiteralKind::Float)
        );
        assert_eq!(output.tokens[4].lexeme, "2.5j");
        assert_eq!(
            output.tokens[6].kind,
            TokenKind::NumberLiteral(NumberLiteralKind::Scientific)
        );
        assert_eq!(output.tokens[6].lexeme, "3e-2i");
    }

    #[test]
    fn lexes_transpose_operators_after_expressions() {
        let output = lex("x = a' + b.'", SourceFileId(1));

        assert!(output.diagnostics.is_empty());
        assert_eq!(
            output.tokens[3].kind,
            TokenKind::Operator(OperatorKind::Transpose)
        );
        assert_eq!(
            output.tokens[6].kind,
            TokenKind::Operator(OperatorKind::DotTranspose)
        );
    }

    #[test]
    fn keeps_char_literals_when_apostrophe_starts_text() {
        let output = lex("x = 'ab'", SourceFileId(1));

        assert!(output.diagnostics.is_empty());
        assert_eq!(output.tokens[2].kind, TokenKind::CharLiteral);
        assert_eq!(output.tokens[2].lexeme, "'ab'");
    }

    #[test]
    fn keeps_char_literals_after_whitespace_boundary_when_quote_closes() {
        let output = lex("x = a 'ab'", SourceFileId(1));

        assert!(output.diagnostics.is_empty());
        assert_eq!(output.tokens[3].kind, TokenKind::CharLiteral);
        assert_eq!(output.tokens[3].lexeme, "'ab'");
    }

    #[test]
    fn line_continuation_becomes_trivia_and_suppresses_newline_token() {
        let output = lex("x = 1 ... comment\n+ 2", SourceFileId(1));

        assert!(output.diagnostics.is_empty());
        assert!(output
            .tokens
            .iter()
            .all(|token| token.kind != TokenKind::Newline));
        assert!(output.tokens[3]
            .leading_trivia
            .iter()
            .any(|trivia| trivia.kind == TriviaKind::LineContinuation));
    }

    #[test]
    fn reports_unterminated_string_literal() {
        let output = lex("\"abc", SourceFileId(1));

        assert_eq!(output.diagnostics.len(), 1);
        assert_eq!(output.diagnostics[0].code, "LEX002");
    }
}
