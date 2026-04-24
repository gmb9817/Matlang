//! Parser entrypoints for the Release 0.1 frontend.

use crate::{
    ast::{
        AssignmentTarget, BinaryOp, ClassDef, ClassMemberAccess, ClassMethodBlock,
        ClassPropertyBlock, ClassPropertyDef, CompilationUnit, CompilationUnitKind,
        ConditionalBranch, Expression, ExpressionKind, FunctionDef, FunctionHandleTarget,
        Identifier, IndexArgument, Item, QualifiedName, Statement, StatementKind, SwitchCase,
        UnaryOp,
    },
    diagnostics::Diagnostic,
    lexer::{lex, DelimiterKind, Keyword, OperatorKind, Token, TokenKind, Trivia, TriviaKind},
    source::{SourceFileId, SourcePosition, SourceSpan},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseMode {
    Script,
    FunctionFile,
    ClassFile,
    AutoDetect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseOutput {
    pub unit: Option<CompilationUnit>,
    pub diagnostics: Vec<Diagnostic>,
}

impl ParseOutput {
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == crate::diagnostics::Severity::Error)
    }
}

pub fn parse_source(source: &str, file_id: SourceFileId, mode: ParseMode) -> ParseOutput {
    let lexed = lex(source, file_id);
    let mut parsed = parse_tokens(&lexed.tokens, mode);
    parsed.diagnostics.splice(0..0, lexed.diagnostics);
    parsed
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandExpressionContext {
    Statement,
    AssignmentRhs,
}

pub fn parse_tokens(tokens: &[Token], mode: ParseMode) -> ParseOutput {
    let parser = Parser::new(tokens, mode);
    parser.parse()
}

struct Parser<'a> {
    tokens: &'a [Token],
    cursor: usize,
    mode: ParseMode,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token], mode: ParseMode) -> Self {
        Self {
            tokens,
            cursor: 0,
            mode,
            diagnostics: Vec::new(),
        }
    }

    fn parse(mut self) -> ParseOutput {
        self.skip_separators();
        let kind = match self.mode {
            ParseMode::Script => CompilationUnitKind::Script,
            ParseMode::FunctionFile => CompilationUnitKind::FunctionFile,
            ParseMode::ClassFile => CompilationUnitKind::ClassFile,
            ParseMode::AutoDetect => {
                if self.at_keyword(Keyword::Function) {
                    CompilationUnitKind::FunctionFile
                } else if self.at_keyword(Keyword::ClassDef) {
                    CompilationUnitKind::ClassFile
                } else {
                    CompilationUnitKind::Script
                }
            }
        };

        let start = self.current().span;
        let mut items = Vec::new();

        match kind {
            CompilationUnitKind::Script | CompilationUnitKind::FunctionFile => {
                while !self.at_end() {
                    self.skip_separators();
                    if self.at_end() {
                        break;
                    }

                    if self.at_keyword(Keyword::Function) {
                        items.push(Item::Function(self.parse_function_definition()));
                    } else {
                        items.push(Item::Statement(self.parse_statement()));
                    }

                    self.skip_separators();
                }
            }
            CompilationUnitKind::ClassFile => {
                if self.at_keyword(Keyword::ClassDef) {
                    items.push(Item::Class(self.parse_class_definition()));
                } else if !self.at_end() {
                    self.error_here("PAR101", "expected `classdef`");
                }

                self.skip_separators();
                while !self.at_end() {
                    self.error_here(
                        "PAR102",
                        "top-level content after a class definition is not supported",
                    );
                    self.advance_if_not_eof();
                    self.skip_separators();
                }
            }
        }

        let span = items
            .first()
            .map(item_span)
            .zip(items.last().map(item_span))
            .map(|(first, last)| combine_spans(first, last))
            .unwrap_or(start);

        ParseOutput {
            unit: Some(CompilationUnit { kind, items, span }),
            diagnostics: self.diagnostics,
        }
    }

    fn parse_function_definition(&mut self) -> FunctionDef {
        let function_span = self.expect_keyword(Keyword::Function, "PAR001", "expected `function`");
        let mut outputs = Vec::new();

        if self.at_identifier() && self.peek_operator(OperatorKind::Assign) {
            outputs.push(self.parse_identifier());
            self.expect_operator(
                OperatorKind::Assign,
                "PAR002",
                "expected `=` after output name",
            );
        } else if self.looks_like_output_list() {
            self.expect_delimiter(DelimiterKind::LeftBracket, "PAR003", "expected `[`");
            outputs = self.parse_identifier_list(DelimiterKind::RightBracket);
            self.expect_delimiter(
                DelimiterKind::RightBracket,
                "PAR004",
                "expected `]` after output list",
            );
            self.expect_operator(
                OperatorKind::Assign,
                "PAR005",
                "expected `=` after output list",
            );
        }

        let name = self.parse_identifier_or_recover("PAR006", "expected function name");
        let inputs = if self.match_delimiter(DelimiterKind::LeftParen) {
            let parsed = self.parse_identifier_list(DelimiterKind::RightParen);
            self.expect_delimiter(
                DelimiterKind::RightParen,
                "PAR007",
                "expected `)` after parameter list",
            );
            parsed
        } else {
            Vec::new()
        };

        self.skip_separators();
        let mut body = Vec::new();
        let mut local_functions = Vec::new();

        while !self.at_end() && !self.at_keyword(Keyword::End) {
            self.skip_separators();
            if self.at_end() || self.at_keyword(Keyword::End) {
                break;
            }

            if self.at_keyword(Keyword::Function) {
                local_functions.push(self.parse_function_definition());
            } else {
                body.push(self.parse_statement());
            }

            self.skip_separators();
        }

        let end_span =
            self.expect_keyword(Keyword::End, "PAR008", "expected `end` after function body");

        FunctionDef {
            name,
            inputs,
            outputs,
            body,
            local_functions,
            span: combine_spans(function_span, end_span),
        }
    }

    fn parse_class_definition(&mut self) -> ClassDef {
        let class_span = self.expect_keyword(Keyword::ClassDef, "PAR103", "expected `classdef`");
        self.parse_class_definition_attributes();
        let name = self.parse_identifier_or_recover("PAR104", "expected class name");
        let superclass = if self.match_operator(OperatorKind::LessThan) {
            let qualified =
                self.parse_qualified_name("PAR105", "expected superclass name after `<`");
            if self.at_operator(OperatorKind::LogicalAnd) {
                self.error_here("PAR106", "multiple superclasses are not supported");
                while !self.at_end() && !self.at_separator() {
                    self.advance_if_not_eof();
                }
            }
            Some(qualified)
        } else {
            None
        };

        self.skip_separators();
        let mut property_blocks = Vec::new();
        let mut method_blocks = Vec::new();

        while !self.at_end() && !self.at_keyword(Keyword::End) {
            self.skip_separators();
            if self.at_end() || self.at_keyword(Keyword::End) {
                break;
            }

            if self.at_keyword(Keyword::Properties) {
                property_blocks.push(self.parse_class_property_block());
            } else if self.at_keyword(Keyword::Methods) {
                method_blocks.push(self.parse_class_method_block());
            } else if self.current_identifier_is("events") {
                self.parse_unsupported_class_block("events", "PAR107");
            } else if self.current_identifier_is("enumeration") {
                self.parse_unsupported_class_block("enumeration", "PAR108");
            } else if self.at_keyword(Keyword::Function) {
                self.error_here(
                    "PAR109",
                    "methods must appear inside a `methods` block in a class definition",
                );
                self.parse_function_definition();
            } else if self.can_start_class_statement_recovery() {
                self.parse_unsupported_class_statement();
            } else {
                self.error_here("PAR110", "unsupported class body item");
                self.advance_if_not_eof();
            }

            self.skip_separators();
        }

        let end_span =
            self.expect_keyword(Keyword::End, "PAR111", "expected `end` after class body");
        ClassDef {
            name,
            superclass,
            property_blocks,
            method_blocks,
            span: combine_spans(class_span, end_span),
        }
    }

    fn parse_unsupported_class_block(&mut self, block_name: &str, code: &'static str) {
        let start = self.current().span;
        self.diagnostics.push(Diagnostic::error(
            code,
            format!("`{block_name}` blocks are not supported"),
            start,
        ));
        self.advance_if_not_eof();
        self.parse_unsupported_member_attribute_list(block_name, "PAR125", "PAR126");
        self.skip_separators();
        while !self.at_end() && !self.at_keyword(Keyword::End) {
            self.advance_if_not_eof();
        }
        self.expect_keyword(
            Keyword::End,
            "PAR127",
            "expected `end` after unsupported class block",
        );
    }

    fn parse_unsupported_class_statement(&mut self) {
        let span = self.current().span;
        self.diagnostics.push(Diagnostic::error(
            "PAR128",
            "top-level executable statements are not supported in class definitions",
            span,
        ));
        let _ = self.parse_statement();
    }

    fn parse_class_definition_attributes(&mut self) {
        self.skip_trivia_only();
        if !self.at_delimiter(DelimiterKind::LeftParen) {
            return;
        }

        self.advance();
        let mut saw_any = false;
        while !self.at_end() && !self.at_delimiter(DelimiterKind::RightParen) {
            self.skip_newlines_only();
            if self.at_delimiter(DelimiterKind::Comma) {
                self.advance();
                continue;
            }

            saw_any = true;
            if self.at_identifier() {
                let attribute = self.parse_identifier();
                self.diagnostics.push(Diagnostic::error(
                    "PAR122",
                    format!(
                        "`classdef` attribute `{}` is not supported in the current parser",
                        attribute.name
                    ),
                    attribute.span,
                ));
            } else {
                let span = self.current().span;
                self.diagnostics.push(Diagnostic::error(
                    "PAR122",
                    "expected a `classdef` attribute name",
                    span,
                ));
                self.advance_if_not_eof();
            }

            self.skip_newlines_only();
            if self.match_operator(OperatorKind::Assign) {
                self.consume_attribute_value();
            }
            self.skip_newlines_only();
            if self.at_delimiter(DelimiterKind::Comma) {
                self.advance();
            }
        }

        self.expect_delimiter(
            DelimiterKind::RightParen,
            "PAR123",
            "expected `)` after classdef attribute list",
        );
        if !saw_any {
            self.diagnostics.push(Diagnostic::error(
                "PAR124",
                "empty `classdef` attribute lists are not supported",
                self.current().span,
            ));
        }
    }

    fn parse_unsupported_member_attribute_list(
        &mut self,
        context_name: &str,
        code: &'static str,
        empty_code: &'static str,
    ) {
        self.skip_trivia_only();
        if !self.at_delimiter(DelimiterKind::LeftParen) {
            return;
        }

        self.advance();
        let mut saw_any = false;
        while !self.at_end() && !self.at_delimiter(DelimiterKind::RightParen) {
            self.skip_newlines_only();
            if self.at_delimiter(DelimiterKind::Comma) {
                self.advance();
                continue;
            }

            saw_any = true;
            if self.at_identifier() {
                let attribute = self.parse_identifier();
                self.diagnostics.push(Diagnostic::error(
                    code,
                    format!(
                        "`{context_name}` block attribute `{}` is not supported in the current parser",
                        attribute.name
                    ),
                    attribute.span,
                ));
            } else {
                let span = self.current().span;
                self.diagnostics.push(Diagnostic::error(
                    code,
                    format!("expected a `{context_name}` block attribute name"),
                    span,
                ));
                self.advance_if_not_eof();
            }

            self.skip_newlines_only();
            if self.match_operator(OperatorKind::Assign) {
                self.consume_attribute_value();
            }
            self.skip_newlines_only();
            if self.at_delimiter(DelimiterKind::Comma) {
                self.advance();
            }
        }

        self.expect_delimiter(
            DelimiterKind::RightParen,
            code,
            "expected `)` after unsupported class block attributes",
        );
        if !saw_any {
            self.diagnostics.push(Diagnostic::error(
                empty_code,
                format!("empty `{context_name}` attribute lists are not supported"),
                self.current().span,
            ));
        }
    }

    fn consume_attribute_value(&mut self) {
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut brace_depth = 0usize;

        while !self.at_end() {
            match self.current().kind {
                TokenKind::Delimiter(DelimiterKind::LeftParen) => paren_depth += 1,
                TokenKind::Delimiter(DelimiterKind::RightParen) => {
                    if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 {
                        break;
                    }
                    paren_depth = paren_depth.saturating_sub(1);
                }
                TokenKind::Delimiter(DelimiterKind::LeftBracket) => bracket_depth += 1,
                TokenKind::Delimiter(DelimiterKind::RightBracket) => {
                    bracket_depth = bracket_depth.saturating_sub(1);
                }
                TokenKind::Delimiter(DelimiterKind::LeftBrace) => brace_depth += 1,
                TokenKind::Delimiter(DelimiterKind::RightBrace) => {
                    brace_depth = brace_depth.saturating_sub(1);
                }
                TokenKind::Delimiter(DelimiterKind::Comma)
                    if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 =>
                {
                    break;
                }
                _ => {}
            }
            self.advance_if_not_eof();
        }
    }

    fn parse_class_property_block(&mut self) -> ClassPropertyBlock {
        let start = self.expect_keyword(Keyword::Properties, "PAR112", "expected `properties`");
        let access = self.parse_class_property_block_attributes();
        self.skip_separators();

        let mut properties = Vec::new();
        while !self.at_end() && !self.at_keyword(Keyword::End) {
            self.skip_separators();
            if self.at_end() || self.at_keyword(Keyword::End) {
                break;
            }
            let name = self.parse_identifier_or_recover("PAR114", "expected property name");
            let default = if self.match_operator(OperatorKind::Assign) {
                Some(self.parse_expression())
            } else {
                None
            };
            if !self.at_separator() && !self.at_keyword(Keyword::End) {
                self.diagnostics.push(Diagnostic::error(
                    "PAR129",
                    "property validation, size/type constraints, and trailing declaration syntax are not supported in the current parser",
                    self.current().span,
                ));
                self.consume_class_property_declaration_tail();
            }
            let span = default
                .as_ref()
                .map(|value| combine_spans(name.span, value.span))
                .unwrap_or(name.span);
            properties.push(ClassPropertyDef {
                name,
                default,
                span,
            });
            if self.at_delimiter(DelimiterKind::Semicolon) {
                self.advance();
            }
            self.skip_separators();
        }

        let end = self.expect_keyword(
            Keyword::End,
            "PAR115",
            "expected `end` after properties block",
        );
        ClassPropertyBlock {
            access,
            properties,
            span: combine_spans(start, end),
        }
    }

    fn consume_class_property_declaration_tail(&mut self) {
        while !self.at_end() && !self.at_separator() && !self.at_keyword(Keyword::End) {
            self.advance_if_not_eof();
        }
    }

    fn consume_class_member_declaration_tail(&mut self) {
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut brace_depth = 0usize;

        while !self.at_end() {
            match self.current().kind {
                TokenKind::Delimiter(DelimiterKind::LeftParen) => paren_depth += 1,
                TokenKind::Delimiter(DelimiterKind::RightParen) => {
                    paren_depth = paren_depth.saturating_sub(1);
                }
                TokenKind::Delimiter(DelimiterKind::LeftBracket) => bracket_depth += 1,
                TokenKind::Delimiter(DelimiterKind::RightBracket) => {
                    bracket_depth = bracket_depth.saturating_sub(1);
                }
                TokenKind::Delimiter(DelimiterKind::LeftBrace) => brace_depth += 1,
                TokenKind::Delimiter(DelimiterKind::RightBrace) => {
                    brace_depth = brace_depth.saturating_sub(1);
                }
                TokenKind::Newline | TokenKind::Delimiter(DelimiterKind::Semicolon)
                    if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 =>
                {
                    break;
                }
                TokenKind::Keyword(Keyword::End)
                    if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 =>
                {
                    break;
                }
                _ => {}
            }
            self.advance_if_not_eof();
        }
    }

    fn parse_class_method_block(&mut self) -> ClassMethodBlock {
        let start = self.expect_keyword(Keyword::Methods, "PAR116", "expected `methods`");
        let (is_static, access) = self.parse_class_method_block_attributes();
        self.skip_separators();

        let mut methods = Vec::new();
        while !self.at_end() && !self.at_keyword(Keyword::End) {
            self.skip_separators();
            if self.at_end() || self.at_keyword(Keyword::End) {
                break;
            }
            if self.at_keyword(Keyword::Function) {
                methods.push(self.parse_function_definition());
            } else {
                self.diagnostics.push(Diagnostic::error(
                    "PAR118",
                    "methods blocks currently support only full `function ... end` method definitions; signature-only or abstract declarations are not supported",
                    self.current().span,
                ));
                self.consume_class_member_declaration_tail();
            }
            self.skip_separators();
        }

        let end = self.expect_keyword(Keyword::End, "PAR119", "expected `end` after methods block");
        ClassMethodBlock {
            access,
            is_static,
            methods,
            span: combine_spans(start, end),
        }
    }

    fn parse_class_property_block_attributes(&mut self) -> ClassMemberAccess {
        self.skip_trivia_only();
        if !self.at_delimiter(DelimiterKind::LeftParen) {
            return ClassMemberAccess::Public;
        }

        self.advance();
        let mut access = ClassMemberAccess::Public;
        let mut saw_any = false;
        while !self.at_end() && !self.at_delimiter(DelimiterKind::RightParen) {
            if self.at_delimiter(DelimiterKind::Comma) {
                self.advance();
                continue;
            }

            if self.current_identifier_is("Access") {
                saw_any = true;
                self.advance();
                self.expect_operator(
                    OperatorKind::Assign,
                    "PAR117",
                    "expected `=` after `Access`",
                );
                if self.current_identifier_is("private") {
                    access = ClassMemberAccess::Private;
                    self.advance();
                } else if self.current_identifier_is("public") {
                    access = ClassMemberAccess::Public;
                    self.advance();
                } else {
                    self.error_here(
                        "PAR117",
                        "only `Access=private` or `Access=public` is supported",
                    );
                    self.consume_attribute_value();
                }
                continue;
            }

            if self.at_identifier() {
                saw_any = true;
                let attribute = self.parse_identifier();
                self.diagnostics.push(Diagnostic::error(
                    "PAR117",
                    format!(
                        "property block attribute `{}` is not supported in the current parser",
                        attribute.name
                    ),
                    attribute.span,
                ));
                if self.match_operator(OperatorKind::Assign) {
                    self.consume_attribute_value();
                }
                continue;
            }

            saw_any = true;
            self.error_here("PAR117", "expected a property block attribute name");
            self.advance_if_not_eof();
        }

        self.expect_delimiter(
            DelimiterKind::RightParen,
            "PAR120",
            "expected `)` after properties block attributes",
        );
        if !saw_any {
            self.error_here(
                "PAR121",
                "empty properties attribute lists are not supported",
            );
        }
        access
    }

    fn parse_class_method_block_attributes(&mut self) -> (bool, ClassMemberAccess) {
        self.skip_trivia_only();
        if !self.at_delimiter(DelimiterKind::LeftParen) {
            return (false, ClassMemberAccess::Public);
        }

        self.advance();
        let mut is_static = false;
        let mut access = ClassMemberAccess::Public;
        let mut saw_any = false;
        while !self.at_end() && !self.at_delimiter(DelimiterKind::RightParen) {
            if self.at_delimiter(DelimiterKind::Comma) {
                self.advance();
                continue;
            }

            if self.current_identifier_is("Static") {
                saw_any = true;
                self.advance();
                if self.match_operator(OperatorKind::Assign) {
                    if self.current_identifier_is("true") {
                        is_static = true;
                        self.advance();
                    } else {
                        self.error_here(
                            "PAR117",
                            "`Static=true` is the only supported explicit Static form",
                        );
                        self.consume_attribute_value();
                    }
                } else {
                    is_static = true;
                }
                continue;
            }

            if self.current_identifier_is("Access") {
                saw_any = true;
                self.advance();
                self.expect_operator(
                    OperatorKind::Assign,
                    "PAR117",
                    "expected `=` after `Access`",
                );
                if self.current_identifier_is("private") {
                    access = ClassMemberAccess::Private;
                    self.advance();
                } else if self.current_identifier_is("public") {
                    access = ClassMemberAccess::Public;
                    self.advance();
                } else {
                    self.error_here(
                        "PAR117",
                        "only `Access=private` or `Access=public` is supported",
                    );
                    self.consume_attribute_value();
                }
                continue;
            }

            if self.at_identifier() {
                saw_any = true;
                let attribute = self.parse_identifier();
                self.diagnostics.push(Diagnostic::error(
                    "PAR117",
                    format!(
                        "method block attribute `{}` is not supported in the current parser",
                        attribute.name
                    ),
                    attribute.span,
                ));
                if self.match_operator(OperatorKind::Assign) {
                    self.consume_attribute_value();
                }
                continue;
            }

            saw_any = true;
            self.error_here("PAR117", "expected a method block attribute name");
            self.advance_if_not_eof();
        }

        self.expect_delimiter(
            DelimiterKind::RightParen,
            "PAR120",
            "expected `)` after methods block attributes",
        );
        if !saw_any {
            self.error_here("PAR121", "empty methods attribute lists are not supported");
        }
        (is_static, access)
    }

    fn parse_statement_block(&mut self, terminators: &[Keyword]) -> Vec<Statement> {
        let mut body = Vec::new();

        while !self.at_end() {
            self.skip_separators();
            if self.at_end() || terminators.iter().any(|keyword| self.at_keyword(*keyword)) {
                break;
            }

            body.push(self.parse_statement());
            self.skip_separators();
        }

        body
    }

    fn parse_statement(&mut self) -> Statement {
        let mut statement = if self.at_keyword(Keyword::If) {
            self.parse_if_statement()
        } else if self.at_keyword(Keyword::Switch) {
            self.parse_switch_statement()
        } else if self.at_keyword(Keyword::Try) {
            self.parse_try_statement()
        } else if self.at_keyword(Keyword::For) {
            self.parse_for_statement()
        } else if self.at_keyword(Keyword::While) {
            self.parse_while_statement()
        } else if self.looks_like_multi_assignment() {
            let start = self.current().span;
            let targets = self.parse_multi_assignment_targets();
            self.expect_operator(
                OperatorKind::Assign,
                "PAR009",
                "expected `=` after assignment targets",
            );
            let value =
                if self.looks_like_command_expression(CommandExpressionContext::AssignmentRhs) {
                    self.parse_command_expression()
                } else {
                    self.parse_expression()
                };
            Statement {
                kind: StatementKind::Assignment {
                    targets,
                    value: value.clone(),
                    list_assignment: true,
                },
                span: combine_spans(start, value.span),
                display_suppressed: false,
            }
        } else if self.at_keyword(Keyword::Break) {
            let span = self.advance().span;
            Statement {
                kind: StatementKind::Break,
                span,
                display_suppressed: false,
            }
        } else if self.at_keyword(Keyword::Continue) {
            let span = self.advance().span;
            Statement {
                kind: StatementKind::Continue,
                span,
                display_suppressed: false,
            }
        } else if self.at_keyword(Keyword::Return) {
            let span = self.advance().span;
            Statement {
                kind: StatementKind::Return,
                span,
                display_suppressed: false,
            }
        } else if self.at_keyword(Keyword::Global) {
            self.parse_name_list_statement(Keyword::Global, StatementKind::Global)
        } else if self.at_keyword(Keyword::Persistent) {
            self.parse_name_list_statement(Keyword::Persistent, StatementKind::Persistent)
        } else if self.looks_like_command_expression(CommandExpressionContext::Statement) {
            let expression = self.parse_command_expression();
            Statement {
                kind: StatementKind::Expression(expression.clone()),
                span: expression.span,
                display_suppressed: false,
            }
        } else {
            let expression = self.parse_expression();
            if self.match_operator(OperatorKind::Assign) {
                let target = self.expression_to_assignment_target(expression.clone());
                let value = if self
                    .looks_like_command_expression(CommandExpressionContext::AssignmentRhs)
                {
                    self.parse_command_expression()
                } else {
                    self.parse_expression()
                };
                Statement {
                    kind: StatementKind::Assignment {
                        targets: vec![target],
                        value: value.clone(),
                        list_assignment: false,
                    },
                    span: combine_spans(expression.span, value.span),
                    display_suppressed: false,
                }
            } else {
                Statement {
                    kind: StatementKind::Expression(expression.clone()),
                    span: expression.span,
                    display_suppressed: false,
                }
            }
        };
        statement.display_suppressed = self.at_delimiter(DelimiterKind::Semicolon);
        statement
    }

    fn parse_if_statement(&mut self) -> Statement {
        let if_span = self.expect_keyword(Keyword::If, "PAR030", "expected `if`");
        let first_condition = self.parse_expression();
        self.skip_separators();

        let first_body =
            self.parse_statement_block(&[Keyword::ElseIf, Keyword::Else, Keyword::End]);
        let mut branches = vec![ConditionalBranch {
            condition: first_condition.clone(),
            body: first_body,
            span: combine_spans(if_span, first_condition.span),
        }];

        while self.at_keyword(Keyword::ElseIf) {
            let elseif_span = self.expect_keyword(Keyword::ElseIf, "PAR031", "expected `elseif`");
            let condition = self.parse_expression();
            self.skip_separators();
            let body = self.parse_statement_block(&[Keyword::ElseIf, Keyword::Else, Keyword::End]);
            branches.push(ConditionalBranch {
                condition: condition.clone(),
                body,
                span: combine_spans(elseif_span, condition.span),
            });
        }

        let else_body = if self.at_keyword(Keyword::Else) {
            self.expect_keyword(Keyword::Else, "PAR032", "expected `else`");
            self.skip_separators();
            self.parse_statement_block(&[Keyword::End])
        } else {
            Vec::new()
        };

        let end_span = self.expect_keyword(Keyword::End, "PAR033", "expected `end` after `if`");
        Statement {
            kind: StatementKind::If {
                branches,
                else_body,
            },
            span: combine_spans(if_span, end_span),
            display_suppressed: false,
        }
    }

    fn parse_switch_statement(&mut self) -> Statement {
        let switch_span = self.expect_keyword(Keyword::Switch, "PAR040", "expected `switch`");
        let expression = self.parse_expression();
        self.skip_separators();

        let mut cases = Vec::new();
        while self.at_keyword(Keyword::Case) {
            let case_span = self.expect_keyword(Keyword::Case, "PAR041", "expected `case`");
            let matcher = self.parse_expression();
            self.skip_separators();
            let body =
                self.parse_statement_block(&[Keyword::Case, Keyword::Otherwise, Keyword::End]);
            cases.push(SwitchCase {
                matcher: matcher.clone(),
                body,
                span: combine_spans(case_span, matcher.span),
            });
        }

        let otherwise_body = if self.at_keyword(Keyword::Otherwise) {
            self.expect_keyword(Keyword::Otherwise, "PAR042", "expected `otherwise`");
            self.skip_separators();
            self.parse_statement_block(&[Keyword::End])
        } else {
            Vec::new()
        };

        let end_span = self.expect_keyword(Keyword::End, "PAR043", "expected `end` after `switch`");
        Statement {
            kind: StatementKind::Switch {
                expression: expression.clone(),
                cases,
                otherwise_body,
            },
            span: combine_spans(switch_span, end_span),
            display_suppressed: false,
        }
    }

    fn parse_try_statement(&mut self) -> Statement {
        let try_span = self.expect_keyword(Keyword::Try, "PAR045", "expected `try`");
        self.skip_separators();

        let body = self.parse_statement_block(&[Keyword::Catch, Keyword::End]);
        let catch_binding = if self.at_keyword(Keyword::Catch) {
            self.expect_keyword(Keyword::Catch, "PAR046", "expected `catch`");
            let binding = if self.at_identifier() {
                Some(self.parse_identifier())
            } else {
                None
            };
            self.skip_separators();
            (binding, self.parse_statement_block(&[Keyword::End]))
        } else {
            (None, Vec::new())
        };

        let end_span = self.expect_keyword(Keyword::End, "PAR047", "expected `end` after `try`");
        Statement {
            kind: StatementKind::Try {
                body,
                catch_binding: catch_binding.0,
                catch_body: catch_binding.1,
            },
            span: combine_spans(try_span, end_span),
            display_suppressed: false,
        }
    }

    fn parse_for_statement(&mut self) -> Statement {
        let for_span = self.expect_keyword(Keyword::For, "PAR034", "expected `for`");
        let variable = self.parse_identifier_or_recover("PAR035", "expected loop variable");
        self.expect_operator(
            OperatorKind::Assign,
            "PAR036",
            "expected `=` after loop variable",
        );
        let iterable = self.parse_expression();
        self.skip_separators();
        let body = self.parse_statement_block(&[Keyword::End]);
        let end_span = self.expect_keyword(Keyword::End, "PAR037", "expected `end` after `for`");
        Statement {
            kind: StatementKind::For {
                variable,
                iterable: iterable.clone(),
                body,
            },
            span: combine_spans(for_span, end_span),
            display_suppressed: false,
        }
    }

    fn parse_while_statement(&mut self) -> Statement {
        let while_span = self.expect_keyword(Keyword::While, "PAR038", "expected `while`");
        let condition = self.parse_expression();
        self.skip_separators();
        let body = self.parse_statement_block(&[Keyword::End]);
        let end_span = self.expect_keyword(Keyword::End, "PAR039", "expected `end` after `while`");
        Statement {
            kind: StatementKind::While {
                condition: condition.clone(),
                body,
            },
            span: combine_spans(while_span, end_span),
            display_suppressed: false,
        }
    }

    fn parse_name_list_statement(
        &mut self,
        keyword: Keyword,
        ctor: fn(Vec<Identifier>) -> StatementKind,
    ) -> Statement {
        let start = self.expect_keyword(keyword, "PAR010", "expected declaration keyword");
        let names = self.parse_identifier_list_until_separator();
        let end = names.last().map(|name| name.span).unwrap_or(start);
        Statement {
            kind: ctor(names),
            span: combine_spans(start, end),
            display_suppressed: false,
        }
    }

    fn parse_expression(&mut self) -> Expression {
        self.parse_short_circuit_expression()
    }

    fn parse_short_circuit_expression(&mut self) -> Expression {
        let mut expression = self.parse_logical_expression();
        loop {
            let op = if self.match_operator(OperatorKind::ShortCircuitAnd) {
                Some(BinaryOp::ShortCircuitAnd)
            } else if self.match_operator(OperatorKind::ShortCircuitOr) {
                Some(BinaryOp::ShortCircuitOr)
            } else {
                None
            };
            let Some(op) = op else { break };
            let rhs = self.parse_logical_expression();
            expression = binary_expression(op, expression, rhs);
        }
        expression
    }

    fn parse_logical_expression(&mut self) -> Expression {
        let mut expression = self.parse_relational_expression();
        loop {
            let op = if self.match_operator(OperatorKind::LogicalAnd) {
                Some(BinaryOp::LogicalAnd)
            } else if self.match_operator(OperatorKind::LogicalOr) {
                Some(BinaryOp::LogicalOr)
            } else {
                None
            };
            let Some(op) = op else { break };
            let rhs = self.parse_relational_expression();
            expression = binary_expression(op, expression, rhs);
        }
        expression
    }

    fn parse_relational_expression(&mut self) -> Expression {
        let mut expression = self.parse_colon_expression();
        loop {
            let op = match self.current().kind {
                TokenKind::Operator(OperatorKind::LessThan) => Some(BinaryOp::LessThan),
                TokenKind::Operator(OperatorKind::LessThanOrEqual) => {
                    Some(BinaryOp::LessThanOrEqual)
                }
                TokenKind::Operator(OperatorKind::GreaterThan) => Some(BinaryOp::GreaterThan),
                TokenKind::Operator(OperatorKind::GreaterThanOrEqual) => {
                    Some(BinaryOp::GreaterThanOrEqual)
                }
                TokenKind::Operator(OperatorKind::Equal) => Some(BinaryOp::Equal),
                TokenKind::Operator(OperatorKind::NotEqual) => Some(BinaryOp::NotEqual),
                _ => None,
            };
            let Some(op) = op else { break };
            self.advance();
            let rhs = self.parse_colon_expression();
            expression = binary_expression(op, expression, rhs);
        }
        expression
    }

    fn parse_colon_expression(&mut self) -> Expression {
        let start = self.parse_additive_expression();
        if !self.match_operator(OperatorKind::Colon) {
            return start;
        }

        let middle = self.parse_additive_expression();
        if self.match_operator(OperatorKind::Colon) {
            let end = self.parse_additive_expression();
            Expression {
                span: combine_spans(start.span, end.span),
                kind: ExpressionKind::Range {
                    start: Box::new(start),
                    step: Some(Box::new(middle)),
                    end: Box::new(end),
                },
            }
        } else {
            Expression {
                span: combine_spans(start.span, middle.span),
                kind: ExpressionKind::Range {
                    start: Box::new(start),
                    step: None,
                    end: Box::new(middle),
                },
            }
        }
    }

    fn parse_additive_expression(&mut self) -> Expression {
        let mut expression = self.parse_multiplicative_expression();
        loop {
            let op = if self.match_operator(OperatorKind::Plus) {
                Some(BinaryOp::Add)
            } else if self.match_operator(OperatorKind::Minus) {
                Some(BinaryOp::Subtract)
            } else {
                None
            };
            let Some(op) = op else { break };
            let rhs = self.parse_multiplicative_expression();
            expression = binary_expression(op, expression, rhs);
        }
        expression
    }

    fn parse_multiplicative_expression(&mut self) -> Expression {
        let mut expression = self.parse_power_expression();
        loop {
            let op = match self.current().kind {
                TokenKind::Operator(OperatorKind::Multiply) => Some(BinaryOp::Multiply),
                TokenKind::Operator(OperatorKind::RightDivide) => Some(BinaryOp::MatrixRightDivide),
                TokenKind::Operator(OperatorKind::LeftDivide) => Some(BinaryOp::MatrixLeftDivide),
                TokenKind::Operator(OperatorKind::ElementwiseMultiply) => {
                    Some(BinaryOp::ElementwiseMultiply)
                }
                TokenKind::Operator(OperatorKind::ElementwiseRightDivide) => {
                    Some(BinaryOp::ElementwiseRightDivide)
                }
                TokenKind::Operator(OperatorKind::ElementwiseLeftDivide) => {
                    Some(BinaryOp::ElementwiseLeftDivide)
                }
                _ => None,
            };
            let Some(op) = op else { break };
            self.advance();
            let rhs = self.parse_power_expression();
            expression = binary_expression(op, expression, rhs);
        }
        expression
    }

    fn parse_power_expression(&mut self) -> Expression {
        let lhs = self.parse_unary_expression();
        let op = match self.current().kind {
            TokenKind::Operator(OperatorKind::Power) => Some(BinaryOp::Power),
            TokenKind::Operator(OperatorKind::ElementwisePower) => Some(BinaryOp::ElementwisePower),
            _ => None,
        };
        let Some(op) = op else { return lhs };
        self.advance();
        let rhs = self.parse_power_expression();
        binary_expression(op, lhs, rhs)
    }

    fn parse_unary_expression(&mut self) -> Expression {
        let op = match self.current().kind {
            TokenKind::Operator(OperatorKind::Plus) => Some(UnaryOp::Plus),
            TokenKind::Operator(OperatorKind::Minus) => Some(UnaryOp::Minus),
            TokenKind::Operator(OperatorKind::LogicalNot) => Some(UnaryOp::LogicalNot),
            _ => None,
        };
        let Some(op) = op else {
            return self.parse_postfix_expression();
        };
        let start = self.advance().span;
        let rhs = self.parse_unary_expression();
        Expression {
            span: combine_spans(start, rhs.span),
            kind: ExpressionKind::Unary {
                op,
                rhs: Box::new(rhs),
            },
        }
    }

    fn parse_postfix_expression(&mut self) -> Expression {
        let expression = self.parse_primary_expression();
        self.parse_postfix_suffixes(expression, true)
    }

    fn parse_postfix_suffixes(
        &mut self,
        mut expression: Expression,
        allow_transpose: bool,
    ) -> Expression {
        loop {
            if self.match_delimiter(DelimiterKind::LeftParen) {
                let indices = self.parse_index_argument_list(DelimiterKind::RightParen);
                let end = self.expect_delimiter(
                    DelimiterKind::RightParen,
                    "PAR011",
                    "expected `)` after argument list",
                );
                expression = Expression {
                    span: combine_spans(expression.span, end),
                    kind: ExpressionKind::ParenApply {
                        target: Box::new(expression),
                        indices,
                    },
                };
                continue;
            }

            if self.match_delimiter(DelimiterKind::LeftBrace) {
                let indices = self.parse_index_argument_list(DelimiterKind::RightBrace);
                let end = self.expect_delimiter(
                    DelimiterKind::RightBrace,
                    "PAR012",
                    "expected `}` after cell index list",
                );
                expression = Expression {
                    span: combine_spans(expression.span, end),
                    kind: ExpressionKind::CellIndex {
                        target: Box::new(expression),
                        indices,
                    },
                };
                continue;
            }

            if self.match_operator(OperatorKind::Dot) {
                let field =
                    self.parse_identifier_or_recover("PAR013", "expected field name after `.`");
                let span = combine_spans(expression.span, field.span);
                expression = Expression {
                    span,
                    kind: ExpressionKind::FieldAccess {
                        target: Box::new(expression),
                        field,
                    },
                };
                continue;
            }

            if allow_transpose && self.at_operator(OperatorKind::DotTranspose) {
                let end = self.advance().span;
                expression = Expression {
                    span: combine_spans(expression.span, end),
                    kind: ExpressionKind::Unary {
                        op: UnaryOp::DotTranspose,
                        rhs: Box::new(expression),
                    },
                };
                continue;
            }

            if allow_transpose && self.at_operator(OperatorKind::Transpose) {
                let end = self.advance().span;
                expression = Expression {
                    span: combine_spans(expression.span, end),
                    kind: ExpressionKind::Unary {
                        op: UnaryOp::Transpose,
                        rhs: Box::new(expression),
                    },
                };
                continue;
            }

            break;
        }

        expression
    }

    fn parse_primary_expression(&mut self) -> Expression {
        match &self.current().kind {
            TokenKind::Identifier
            | TokenKind::Keyword(Keyword::Methods)
            | TokenKind::Keyword(Keyword::Properties) => {
                let identifier = self.parse_identifier();
                Expression {
                    span: identifier.span,
                    kind: ExpressionKind::Identifier(identifier),
                }
            }
            TokenKind::NumberLiteral(_) => {
                let token = self.advance().clone();
                Expression {
                    span: token.span,
                    kind: ExpressionKind::NumberLiteral(token.lexeme),
                }
            }
            TokenKind::CharLiteral => {
                let token = self.advance().clone();
                Expression {
                    span: token.span,
                    kind: ExpressionKind::CharLiteral(token.lexeme),
                }
            }
            TokenKind::StringLiteral => {
                let token = self.advance().clone();
                Expression {
                    span: token.span,
                    kind: ExpressionKind::StringLiteral(token.lexeme),
                }
            }
            TokenKind::Keyword(Keyword::End) => {
                let span = self.advance().span;
                Expression {
                    span,
                    kind: ExpressionKind::EndKeyword,
                }
            }
            TokenKind::Delimiter(DelimiterKind::LeftParen) => self.parse_parenthesized_expression(),
            TokenKind::Delimiter(DelimiterKind::LeftBracket) => {
                self.parse_matrix_literal_expression()
            }
            TokenKind::Delimiter(DelimiterKind::LeftBrace) => self.parse_cell_literal_expression(),
            TokenKind::Operator(OperatorKind::FunctionHandle) => {
                self.parse_function_handle_expression()
            }
            _ => {
                let span = self.current().span;
                self.error_here("PAR014", "expected expression");
                self.advance_if_not_eof();
                Expression {
                    span,
                    kind: ExpressionKind::Identifier(Identifier {
                        name: "<error>".to_string(),
                        span,
                    }),
                }
            }
        }
    }

    fn parse_parenthesized_expression(&mut self) -> Expression {
        let start = self.expect_delimiter(DelimiterKind::LeftParen, "PAR015", "expected `(`");
        let mut inner = self.parse_expression();
        let end = self.expect_delimiter(DelimiterKind::RightParen, "PAR016", "expected `)`");
        inner.span = combine_spans(start, end);
        inner
    }

    fn parse_matrix_literal_expression(&mut self) -> Expression {
        let start = self.expect_delimiter(DelimiterKind::LeftBracket, "PAR017", "expected `[`");
        let rows = self.parse_row_major_expression_grid(DelimiterKind::RightBracket);
        let end = self.expect_delimiter(DelimiterKind::RightBracket, "PAR018", "expected `]`");
        Expression {
            span: combine_spans(start, end),
            kind: ExpressionKind::MatrixLiteral(rows),
        }
    }

    fn parse_cell_literal_expression(&mut self) -> Expression {
        let start = self.expect_delimiter(DelimiterKind::LeftBrace, "PAR019", "expected `{`");
        let rows = self.parse_row_major_expression_grid(DelimiterKind::RightBrace);
        let end = self.expect_delimiter(DelimiterKind::RightBrace, "PAR020", "expected `}`");
        Expression {
            span: combine_spans(start, end),
            kind: ExpressionKind::CellLiteral(rows),
        }
    }

    fn parse_function_handle_expression(&mut self) -> Expression {
        let start = self.expect_operator(OperatorKind::FunctionHandle, "PAR021", "expected `@`");
        if self.match_delimiter(DelimiterKind::LeftParen) {
            let params = self.parse_identifier_list(DelimiterKind::RightParen);
            self.expect_delimiter(
                DelimiterKind::RightParen,
                "PAR022",
                "expected `)` after anonymous function parameters",
            );
            let body = self.parse_expression();
            return Expression {
                span: combine_spans(start, body.span),
                kind: ExpressionKind::AnonymousFunction {
                    params,
                    body: Box::new(body),
                },
            };
        }

        let target = self.parse_function_handle_target();
        Expression {
            span: combine_spans(start, function_handle_target_span(&target)),
            kind: ExpressionKind::FunctionHandle(target),
        }
    }

    fn parse_function_handle_target(&mut self) -> FunctionHandleTarget {
        let identifier = self.parse_identifier_or_recover(
            "PAR023",
            "expected function name or receiver expression after `@`",
        );
        let expression = self.parse_function_handle_target_expression(Expression {
            span: identifier.span,
            kind: ExpressionKind::Identifier(identifier),
        });
        expression_as_qualified_name(&expression)
            .map(FunctionHandleTarget::Name)
            .unwrap_or_else(|| FunctionHandleTarget::Expression(Box::new(expression)))
    }

    fn parse_function_handle_target_expression(
        &mut self,
        mut expression: Expression,
    ) -> Expression {
        loop {
            if self.at_delimiter(DelimiterKind::LeftParen)
                && self.function_handle_index_continues_receiver_chain(
                    DelimiterKind::LeftParen,
                    DelimiterKind::RightParen,
                )
            {
                self.advance();
                let indices = self.parse_index_argument_list(DelimiterKind::RightParen);
                let end = self.expect_delimiter(
                    DelimiterKind::RightParen,
                    "PAR011",
                    "expected `)` after argument list",
                );
                expression = Expression {
                    span: combine_spans(expression.span, end),
                    kind: ExpressionKind::ParenApply {
                        target: Box::new(expression),
                        indices,
                    },
                };
                continue;
            }

            if self.at_delimiter(DelimiterKind::LeftBrace)
                && self.function_handle_index_continues_receiver_chain(
                    DelimiterKind::LeftBrace,
                    DelimiterKind::RightBrace,
                )
            {
                self.advance();
                let indices = self.parse_index_argument_list(DelimiterKind::RightBrace);
                let end = self.expect_delimiter(
                    DelimiterKind::RightBrace,
                    "PAR012",
                    "expected `}` after cell index list",
                );
                expression = Expression {
                    span: combine_spans(expression.span, end),
                    kind: ExpressionKind::CellIndex {
                        target: Box::new(expression),
                        indices,
                    },
                };
                continue;
            }

            if self.match_operator(OperatorKind::Dot) {
                let field =
                    self.parse_identifier_or_recover("PAR013", "expected field name after `.`");
                let span = combine_spans(expression.span, field.span);
                expression = Expression {
                    span,
                    kind: ExpressionKind::FieldAccess {
                        target: Box::new(expression),
                        field,
                    },
                };
                continue;
            }

            break;
        }

        expression
    }

    fn parse_qualified_name(&mut self, code: &'static str, message: &'static str) -> QualifiedName {
        let first = self.parse_identifier_or_recover(code, message);
        let mut span = first.span;
        let mut segments = vec![first];

        while self.match_operator(OperatorKind::Dot) {
            let segment = self.parse_identifier_or_recover(
                "PAR044",
                "expected identifier after `.` in qualified name",
            );
            span = combine_spans(span, segment.span);
            segments.push(segment);
        }

        QualifiedName { segments, span }
    }

    fn parse_row_major_expression_grid(&mut self, closing: DelimiterKind) -> Vec<Vec<Expression>> {
        let mut rows = Vec::new();
        while !self.at_end() && !self.at_delimiter(closing) {
            self.skip_newlines_only();
            if self.at_delimiter(closing) {
                break;
            }

            let first = self.parse_expression();
            let mut row = vec![first.clone()];
            let mut previous_end = first.span.end;
            loop {
                if self.match_delimiter(DelimiterKind::Comma) {
                    let expression = self.parse_expression();
                    previous_end = expression.span.end;
                    row.push(expression);
                    continue;
                }

                if self.at_delimiter(DelimiterKind::Semicolon)
                    || self.at_newline()
                    || self.at_delimiter(closing)
                {
                    break;
                }

                if self.current_starts_implicit_matrix_column(previous_end) {
                    let expression = self.parse_expression();
                    previous_end = expression.span.end;
                    row.push(expression);
                    continue;
                }

                break;
            }
            rows.push(row);

            if self.match_delimiter(DelimiterKind::Semicolon) || self.match_newline() {
                continue;
            }
            if !self.at_delimiter(closing) {
                self.error_here("PAR024", "expected row separator or closing delimiter");
                break;
            }
        }
        rows
    }

    fn current_starts_implicit_matrix_column(&self, previous_end: SourcePosition) -> bool {
        self.can_start_expression()
            && self.current().span.start.line == previous_end.line
            && self.current().span.start.offset > previous_end.offset
    }

    fn parse_index_argument_list(&mut self, closing: DelimiterKind) -> Vec<IndexArgument> {
        let mut args = Vec::new();
        while !self.at_end() && !self.at_delimiter(closing) {
            if self.match_operator(OperatorKind::Colon) {
                args.push(IndexArgument::FullSlice);
            } else if self.at_keyword(Keyword::End) && self.peek_is_index_argument_terminator() {
                self.advance();
                args.push(IndexArgument::End);
            } else {
                args.push(IndexArgument::Expression(self.parse_expression()));
            }

            if !self.match_delimiter(DelimiterKind::Comma) {
                break;
            }
        }
        args
    }

    fn parse_identifier_list(&mut self, closing: DelimiterKind) -> Vec<Identifier> {
        let mut names = Vec::new();
        while !self.at_end() && !self.at_delimiter(closing) {
            names.push(self.parse_identifier_or_recover("PAR025", "expected identifier"));
            if !self.match_delimiter(DelimiterKind::Comma) {
                break;
            }
        }
        names
    }

    fn parse_identifier_list_until_separator(&mut self) -> Vec<Identifier> {
        let mut names = Vec::new();
        while !self.at_end() && !self.at_separator() {
            if !self.at_identifier() {
                break;
            }
            names.push(self.parse_identifier());
            if !self.match_delimiter(DelimiterKind::Comma) {
                break;
            }
        }
        names
    }

    fn parse_multi_assignment_targets(&mut self) -> Vec<AssignmentTarget> {
        self.expect_delimiter(DelimiterKind::LeftBracket, "PAR026", "expected `[`");
        let mut targets = Vec::new();
        while !self.at_end() && !self.at_delimiter(DelimiterKind::RightBracket) {
            let expression = self.parse_expression();
            targets.push(self.expression_to_assignment_target(expression));
            if !self.match_delimiter(DelimiterKind::Comma) {
                break;
            }
        }
        self.expect_delimiter(
            DelimiterKind::RightBracket,
            "PAR028",
            "expected `]` in assignment target list",
        );
        targets
    }

    fn looks_like_output_list(&self) -> bool {
        self.looks_like_bracketed_target_list_followed_by(OperatorKind::Assign)
    }

    fn looks_like_multi_assignment(&self) -> bool {
        self.looks_like_bracketed_target_list_followed_by(OperatorKind::Assign)
    }

    fn looks_like_bracketed_target_list_followed_by(&self, op: OperatorKind) -> bool {
        if !self.at_delimiter(DelimiterKind::LeftBracket) {
            return false;
        }

        let mut index = self.cursor + 1;
        let mut depth = 1i32;
        let mut saw_content = false;
        while let Some(token) = self.tokens.get(index) {
            match token.kind {
                TokenKind::Delimiter(DelimiterKind::LeftParen)
                | TokenKind::Delimiter(DelimiterKind::LeftBracket)
                | TokenKind::Delimiter(DelimiterKind::LeftBrace) => {
                    depth += 1;
                    saw_content = true;
                    index += 1;
                }
                TokenKind::Delimiter(DelimiterKind::RightParen)
                | TokenKind::Delimiter(DelimiterKind::RightBrace) => {
                    depth -= 1;
                    if depth <= 0 {
                        return false;
                    }
                    index += 1;
                }
                TokenKind::Delimiter(DelimiterKind::RightBracket) => {
                    depth -= 1;
                    if depth == 0 {
                        return saw_content
                            && matches!(
                                self.tokens.get(index + 1).map(|token| &token.kind),
                                Some(TokenKind::Operator(found)) if *found == op
                            );
                    }
                    if depth < 0 {
                        return false;
                    }
                    index += 1;
                }
                TokenKind::EndOfFile => return false,
                _ => {
                    saw_content = true;
                    index += 1;
                }
            }
        }
        false
    }

    fn expression_to_assignment_target(&mut self, expression: Expression) -> AssignmentTarget {
        match expression.kind {
            ExpressionKind::Identifier(identifier) => AssignmentTarget::Identifier(identifier),
            ExpressionKind::ParenApply { target, indices } => {
                AssignmentTarget::Index { target, indices }
            }
            ExpressionKind::CellIndex { target, indices } => {
                AssignmentTarget::CellIndex { target, indices }
            }
            ExpressionKind::FieldAccess { target, field } => {
                AssignmentTarget::Field { target, field }
            }
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    "PAR029",
                    "invalid assignment target",
                    expression.span,
                ));
                AssignmentTarget::Identifier(Identifier {
                    name: "<error>".to_string(),
                    span: expression.span,
                })
            }
        }
    }

    fn looks_like_command_expression(&self, context: CommandExpressionContext) -> bool {
        self.command_target_end_index().is_some_and(|index| {
            self.tokens.get(index + 1).is_some_and(|token| {
                self.has_command_argument_boundary(token)
                    && self.token_can_start_command_argument_at(index + 1)
                    && match context {
                        CommandExpressionContext::Statement => true,
                        CommandExpressionContext::AssignmentRhs => {
                            !self.assignment_rhs_prefers_expression(index, index + 1)
                        }
                    }
            })
        })
    }

    fn assignment_rhs_prefers_expression(
        &self,
        target_end_index: usize,
        argument_index: usize,
    ) -> bool {
        match self.tokens.get(argument_index).map(|token| &token.kind) {
            Some(TokenKind::Operator(OperatorKind::Plus | OperatorKind::Minus)) => {
                self.assignment_rhs_prefers_expression_for_signed_number(argument_index)
                    || self.assignment_rhs_prefers_expression_for_signed_grouped_expression(
                        target_end_index,
                        argument_index,
                    )
                    || self.assignment_rhs_prefers_expression_for_signed_postfix_expression(
                        target_end_index,
                        argument_index,
                    )
            }
            Some(TokenKind::Operator(
                OperatorKind::Multiply
                | OperatorKind::ElementwiseMultiply
                | OperatorKind::Power
                | OperatorKind::ElementwisePower,
            )) => self.assignment_rhs_prefers_expression_for_short_product_or_power(
                target_end_index,
                argument_index,
            ),
            Some(TokenKind::Operator(
                OperatorKind::RightDivide
                | OperatorKind::LeftDivide
                | OperatorKind::ElementwiseRightDivide
                | OperatorKind::ElementwiseLeftDivide,
            )) => self.assignment_rhs_prefers_expression_for_short_division(
                target_end_index,
                argument_index,
            ),
            _ => false,
        }
    }

    fn assignment_rhs_prefers_expression_for_signed_number(&self, index: usize) -> bool {
        let Some(number) = self.tokens.get(index + 1) else {
            return false;
        };
        if !matches!(number.kind, TokenKind::NumberLiteral(_)) {
            return false;
        }

        !self.has_contiguous_command_argument_suffix(index + 1)
    }

    fn assignment_rhs_prefers_expression_for_signed_grouped_expression(
        &self,
        target_end_index: usize,
        argument_index: usize,
    ) -> bool {
        if !self.assignment_rhs_target_supports_expression_preference(target_end_index) {
            return false;
        }

        let Some(start_index) = argument_index.checked_add(1) else {
            return false;
        };
        let Some(start) = self.tokens.get(start_index) else {
            return false;
        };
        if !matches!(
            start.kind,
            TokenKind::Delimiter(DelimiterKind::LeftParen | DelimiterKind::LeftBracket)
        ) {
            return false;
        }

        let Some(end_index) = self.balanced_command_argument_end_index(start_index) else {
            return false;
        };

        !self.has_contiguous_command_argument_suffix(end_index)
    }

    fn assignment_rhs_prefers_expression_for_signed_postfix_expression(
        &self,
        target_end_index: usize,
        argument_index: usize,
    ) -> bool {
        if !self.assignment_rhs_target_supports_expression_preference(target_end_index) {
            return false;
        }

        let Some(start_index) = argument_index.checked_add(1) else {
            return false;
        };
        let Some(start) = self.tokens.get(start_index) else {
            return false;
        };
        if start.kind != TokenKind::Identifier {
            return false;
        }

        let Some(end_index) = self.postfix_command_argument_end_index(start_index) else {
            return false;
        };

        !self.has_contiguous_command_argument_suffix(end_index)
    }

    fn assignment_rhs_prefers_expression_for_short_product_or_power(
        &self,
        target_end_index: usize,
        argument_index: usize,
    ) -> bool {
        if !self.assignment_rhs_target_supports_expression_preference(target_end_index) {
            return false;
        }

        let Some(rhs_start) = self.tokens.get(argument_index + 1) else {
            return false;
        };
        if rhs_start.kind == TokenKind::Identifier {
            if let Some(end_index) = self.postfix_command_argument_end_index(argument_index + 1) {
                return !self.has_contiguous_command_argument_suffix(end_index);
            }
        }
        if !token_can_start_expression_kind(&rhs_start.kind) {
            return false;
        }

        !self.has_contiguous_command_argument_suffix(argument_index + 1)
    }

    fn assignment_rhs_prefers_expression_for_short_division(
        &self,
        target_end_index: usize,
        argument_index: usize,
    ) -> bool {
        if !self.assignment_rhs_target_supports_expression_preference(target_end_index) {
            return false;
        }

        let Some(rhs_start) = self.tokens.get(argument_index + 1) else {
            return false;
        };
        if rhs_start.kind == TokenKind::Identifier {
            if let Some(end_index) = self.postfix_command_argument_end_index(argument_index + 1) {
                return !self.has_contiguous_command_argument_suffix(end_index);
            }
        }
        if !token_can_start_expression_kind(&rhs_start.kind) {
            return false;
        }

        !self.has_contiguous_command_argument_suffix(argument_index + 1)
    }

    fn has_contiguous_command_argument_suffix(&self, index: usize) -> bool {
        self.tokens.get(index + 1).is_some_and(|next| {
            !command_trivia_has_boundary(&next.leading_trivia)
                && token_can_continue_command_argument(&next.kind)
        })
    }

    fn assignment_rhs_target_supports_expression_preference(
        &self,
        target_end_index: usize,
    ) -> bool {
        self.command_target_end_index() == Some(target_end_index)
    }

    fn balanced_command_argument_end_index(&self, start_index: usize) -> Option<usize> {
        let start = self.tokens.get(start_index)?;
        if !command_argument_opens_group(&start.kind) {
            return None;
        }

        let mut depth = 0usize;
        for (index, token) in self.tokens.iter().enumerate().skip(start_index) {
            if command_argument_opens_group(&token.kind) {
                depth += 1;
                continue;
            }
            if command_argument_closes_group(&token.kind) {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
        }

        None
    }

    fn postfix_command_argument_end_index(&self, start_index: usize) -> Option<usize> {
        let mut index = start_index;
        let mut saw_postfix = false;

        loop {
            let next_index = index.checked_add(1)?;
            let next = self.tokens.get(next_index)?;
            if command_trivia_has_boundary(&next.leading_trivia) {
                break;
            }

            match next.kind {
                TokenKind::Operator(OperatorKind::Dot) => {
                    let field_index = next_index.checked_add(1)?;
                    let field = self.tokens.get(field_index)?;
                    if command_trivia_has_boundary(&field.leading_trivia)
                        || field.kind != TokenKind::Identifier
                    {
                        break;
                    }
                    index = field_index;
                    saw_postfix = true;
                }
                TokenKind::Delimiter(DelimiterKind::LeftParen | DelimiterKind::LeftBrace) => {
                    index = self.balanced_command_argument_end_index(next_index)?;
                    saw_postfix = true;
                }
                _ => break,
            }
        }

        saw_postfix.then_some(index)
    }

    fn parse_command_expression(&mut self) -> Expression {
        let target = self.parse_command_target_expression();
        let mut indices = Vec::new();
        let mut end_span = target.span;

        while self.current_can_be_command_argument() {
            let argument = self.parse_command_argument_expression();
            end_span = argument.span;
            indices.push(IndexArgument::Expression(argument));
            if self.at_delimiter(DelimiterKind::Comma)
                && self.token_can_start_command_argument_at(self.cursor + 1)
            {
                self.advance();
            }
        }

        Expression {
            span: combine_spans(target.span, end_span),
            kind: ExpressionKind::ParenApply {
                target: Box::new(target),
                indices,
            },
        }
    }

    fn current_can_be_command_argument(&self) -> bool {
        self.token_can_start_command_argument_at(self.cursor)
    }

    fn parse_command_target_expression(&mut self) -> Expression {
        let identifier = self.parse_identifier();
        let mut expression = Expression {
            span: identifier.span,
            kind: ExpressionKind::Identifier(identifier),
        };

        while self.at_operator(OperatorKind::Dot)
            && self.tokens.get(self.cursor + 1).is_some_and(|token| {
                token.leading_trivia.is_empty() && token.kind == TokenKind::Identifier
            })
        {
            self.advance();
            let field = self.parse_identifier();
            let span = combine_spans(expression.span, field.span);
            expression = Expression {
                span,
                kind: ExpressionKind::FieldAccess {
                    target: Box::new(expression),
                    field,
                },
            };
        }

        expression
    }

    fn parse_command_argument_expression(&mut self) -> Expression {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::CharLiteral | TokenKind::StringLiteral => {
                self.finish_raw_command_argument(token.span, token.lexeme, 0)
            }
            kind if self.token_can_start_command_argument_kind(&kind, self.cursor - 1) => {
                self.finish_raw_command_argument(token.span, token.lexeme, 0)
            }
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    "PAR044",
                    "invalid command-form argument",
                    token.span,
                ));
                Expression {
                    span: token.span,
                    kind: ExpressionKind::CharLiteral("''".to_string()),
                }
            }
        }
    }

    fn finish_raw_command_argument(
        &mut self,
        start: SourceSpan,
        mut literal: String,
        mut group_depth: usize,
    ) -> Expression {
        let mut end = start;
        while self.command_argument_can_continue_raw_with_group_depth(group_depth) {
            let token_index = self.cursor;
            let token = self.advance().clone();
            if group_depth == 0 && self.command_preserves_leading_trivia_in_raw(token_index) {
                literal.push_str(&leading_trivia_text(&token.leading_trivia));
            }
            end = token.span;
            literal.push_str(&token.lexeme);
            if command_argument_opens_group(&token.kind) {
                group_depth += 1;
            } else if command_argument_closes_group(&token.kind) {
                group_depth = group_depth.saturating_sub(1);
            }
        }
        Expression {
            span: combine_spans(start, end),
            kind: ExpressionKind::CharLiteral(command_argument_literal(&literal)),
        }
    }

    fn command_argument_can_continue_raw_with_group_depth(&self, group_depth: usize) -> bool {
        if self.at_end() || self.at_newline() {
            return false;
        }
        if group_depth == 0 && command_trivia_has_boundary(&self.current().leading_trivia) {
            return self.command_spaced_operator_can_continue_raw_at(self.cursor);
        }

        if matches!(
            self.current().kind,
            TokenKind::Delimiter(DelimiterKind::Comma | DelimiterKind::Semicolon)
        ) {
            if group_depth > 0 {
                return true;
            }
            return self.command_separator_can_continue_raw();
        }

        token_can_continue_command_argument(&self.current().kind)
    }

    fn command_spaced_operator_can_continue_raw_at(&self, index: usize) -> bool {
        self.command_spaced_operator_token_at(index) || self.command_spaced_operator_rhs_at(index)
    }

    fn command_spaced_operator_token_at(&self, index: usize) -> bool {
        let Some(token) = self.tokens.get(index) else {
            return false;
        };
        let TokenKind::Operator(operator) = token.kind else {
            return false;
        };
        if !command_operator_spacing_can_join_raw(operator)
            || !trivia_is_pure_whitespace(&token.leading_trivia)
        {
            return false;
        }
        let Some(next) = self.tokens.get(index + 1) else {
            return false;
        };
        trivia_is_pure_whitespace(&next.leading_trivia)
            && token_can_continue_command_argument(&next.kind)
    }

    fn command_spaced_operator_rhs_at(&self, index: usize) -> bool {
        let Some(token) = self.tokens.get(index) else {
            return false;
        };
        if !trivia_is_pure_whitespace(&token.leading_trivia)
            || !token_can_continue_command_argument(&token.kind)
        {
            return false;
        }
        let Some(previous_index) = index.checked_sub(1) else {
            return false;
        };
        self.command_spaced_operator_token_at(previous_index)
    }

    fn command_preserves_leading_trivia_in_raw(&self, index: usize) -> bool {
        self.command_spaced_operator_can_continue_raw_at(index)
    }

    fn command_separator_can_continue_raw(&self) -> bool {
        !command_trivia_has_boundary(&self.current().leading_trivia)
            && self.tokens.get(self.cursor + 1).is_some_and(|next| {
                !command_trivia_has_boundary(&next.leading_trivia)
                    && token_can_continue_command_argument(&next.kind)
            })
    }

    fn has_command_argument_boundary(&self, token: &Token) -> bool {
        command_trivia_has_boundary(&token.leading_trivia)
    }

    fn command_target_end_index(&self) -> Option<usize> {
        if !self.at_callable_identifier() {
            return None;
        }

        let mut index = self.cursor;
        loop {
            let Some(dot) = self.tokens.get(index + 1) else {
                break;
            };
            let Some(field) = self.tokens.get(index + 2) else {
                break;
            };

            if dot.kind != TokenKind::Operator(OperatorKind::Dot)
                || !dot.leading_trivia.is_empty()
                || field.kind != TokenKind::Identifier
                || !field.leading_trivia.is_empty()
            {
                break;
            }

            index += 2;
        }

        Some(index)
    }

    fn token_can_start_command_argument_at(&self, index: usize) -> bool {
        self.tokens
            .get(index)
            .is_some_and(|token| self.token_can_start_command_argument_kind(&token.kind, index))
    }

    fn token_can_start_command_argument_kind(&self, kind: &TokenKind, index: usize) -> bool {
        if token_can_start_command_argument(kind) {
            return true;
        }

        if matches!(kind, TokenKind::Operator(OperatorKind::Dot)) {
            return true;
        }

        matches!(
            kind,
            TokenKind::Operator(
                OperatorKind::Minus
                    | OperatorKind::Plus
                    | OperatorKind::Multiply
                    | OperatorKind::Power
                    | OperatorKind::ElementwiseMultiply
                    | OperatorKind::ElementwisePower
                    | OperatorKind::RightDivide
                    | OperatorKind::LeftDivide
                    | OperatorKind::ElementwiseRightDivide
                    | OperatorKind::ElementwiseLeftDivide
                    | OperatorKind::FunctionHandle
            )
        ) && self.tokens.get(index + 1).is_some_and(|next| {
            !command_trivia_has_boundary(&next.leading_trivia)
                && token_can_continue_command_argument(&next.kind)
        })
    }

    fn error_here(&mut self, code: &'static str, message: impl Into<String>) {
        self.diagnostics
            .push(Diagnostic::error(code, message, self.current().span));
    }

    fn expect_keyword(
        &mut self,
        keyword: Keyword,
        code: &'static str,
        message: &'static str,
    ) -> SourceSpan {
        if self.at_keyword(keyword) {
            self.advance().span
        } else {
            let span = self.current().span;
            self.diagnostics
                .push(Diagnostic::error(code, message, span));
            span
        }
    }

    fn expect_operator(
        &mut self,
        operator: OperatorKind,
        code: &'static str,
        message: &'static str,
    ) -> SourceSpan {
        if self.at_operator(operator) {
            self.advance().span
        } else {
            let span = self.current().span;
            self.diagnostics
                .push(Diagnostic::error(code, message, span));
            span
        }
    }

    fn expect_delimiter(
        &mut self,
        delimiter: DelimiterKind,
        code: &'static str,
        message: &'static str,
    ) -> SourceSpan {
        if self.at_delimiter(delimiter) {
            self.advance().span
        } else {
            let span = self.current().span;
            self.diagnostics
                .push(Diagnostic::error(code, message, span));
            span
        }
    }

    fn parse_identifier_or_recover(
        &mut self,
        code: &'static str,
        message: &'static str,
    ) -> Identifier {
        if self.at_identifier() {
            self.parse_identifier()
        } else {
            let span = self.current().span;
            self.diagnostics
                .push(Diagnostic::error(code, message, span));
            Identifier {
                name: "<error>".to_string(),
                span,
            }
        }
    }

    fn parse_identifier(&mut self) -> Identifier {
        let token = self.advance().clone();
        Identifier {
            name: token.lexeme,
            span: token.span,
        }
    }

    fn skip_separators(&mut self) {
        while self.at_separator() {
            self.advance();
        }
    }

    fn skip_newlines_only(&mut self) {
        while self.at_newline() {
            self.advance();
        }
    }

    fn skip_trivia_only(&mut self) {}

    fn match_operator(&mut self, operator: OperatorKind) -> bool {
        if self.at_operator(operator) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn match_delimiter(&mut self, delimiter: DelimiterKind) -> bool {
        if self.at_delimiter(delimiter) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn match_newline(&mut self) -> bool {
        if self.at_newline() {
            self.advance();
            true
        } else {
            false
        }
    }

    fn peek_operator(&self, operator: OperatorKind) -> bool {
        matches!(
            self.tokens.get(self.cursor + 1).map(|token| &token.kind),
            Some(TokenKind::Operator(found)) if *found == operator
        )
    }

    fn peek_is_index_argument_terminator(&self) -> bool {
        matches!(
            self.tokens.get(self.cursor + 1).map(|token| &token.kind),
            Some(TokenKind::Delimiter(DelimiterKind::Comma))
                | Some(TokenKind::Delimiter(DelimiterKind::RightParen))
                | Some(TokenKind::Delimiter(DelimiterKind::RightBrace))
        )
    }

    fn at_identifier(&self) -> bool {
        matches!(self.current().kind, TokenKind::Identifier)
    }

    fn at_callable_identifier(&self) -> bool {
        matches!(
            self.current().kind,
            TokenKind::Identifier | TokenKind::Keyword(Keyword::Methods | Keyword::Properties)
        )
    }

    fn current_identifier_is(&self, expected: &str) -> bool {
        matches!(
            &self.current().kind,
            TokenKind::Identifier if self.current().lexeme.eq_ignore_ascii_case(expected)
        )
    }

    fn can_start_class_statement_recovery(&self) -> bool {
        self.can_start_expression()
            || matches!(
                self.current().kind,
                TokenKind::Keyword(
                    Keyword::If
                        | Keyword::Switch
                        | Keyword::Try
                        | Keyword::For
                        | Keyword::While
                        | Keyword::Break
                        | Keyword::Continue
                        | Keyword::Return
                        | Keyword::Global
                        | Keyword::Persistent
                )
            )
    }

    fn can_start_expression(&self) -> bool {
        token_can_start_expression_kind(&self.current().kind)
    }

    fn at_keyword(&self, keyword: Keyword) -> bool {
        matches!(self.current().kind, TokenKind::Keyword(found) if found == keyword)
    }

    fn at_operator(&self, operator: OperatorKind) -> bool {
        matches!(self.current().kind, TokenKind::Operator(found) if found == operator)
    }

    fn at_delimiter(&self, delimiter: DelimiterKind) -> bool {
        matches!(self.current().kind, TokenKind::Delimiter(found) if found == delimiter)
    }

    fn at_newline(&self) -> bool {
        matches!(self.current().kind, TokenKind::Newline)
    }

    fn at_separator(&self) -> bool {
        self.at_newline() || self.at_delimiter(DelimiterKind::Semicolon)
    }

    fn at_end(&self) -> bool {
        matches!(self.current().kind, TokenKind::EndOfFile)
    }

    fn current(&self) -> &Token {
        &self.tokens[self.cursor.min(self.tokens.len().saturating_sub(1))]
    }

    fn advance(&mut self) -> &Token {
        let index = self.cursor.min(self.tokens.len().saturating_sub(1));
        if !self.at_end() {
            self.cursor += 1;
        }
        &self.tokens[index]
    }

    fn advance_if_not_eof(&mut self) {
        if !self.at_end() {
            self.cursor += 1;
        }
    }
}

fn binary_expression(op: BinaryOp, lhs: Expression, rhs: Expression) -> Expression {
    Expression {
        span: combine_spans(lhs.span, rhs.span),
        kind: ExpressionKind::Binary {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        },
    }
}

fn combine_spans(start: SourceSpan, end: SourceSpan) -> SourceSpan {
    SourceSpan::new(start.file_id, start.start, end.end)
}

fn item_span(item: &Item) -> SourceSpan {
    match item {
        Item::Statement(statement) => statement.span,
        Item::Function(function) => function.span,
        Item::Class(class_def) => class_def.span,
    }
}

fn command_argument_literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

fn command_trivia_has_boundary(trivia: &[Trivia]) -> bool {
    trivia
        .iter()
        .any(|trivia| trivia.kind != TriviaKind::LineContinuation)
}

fn trivia_is_pure_whitespace(trivia: &[Trivia]) -> bool {
    !trivia.is_empty()
        && trivia
            .iter()
            .all(|trivia| trivia.kind == TriviaKind::Whitespace)
}

fn leading_trivia_text(trivia: &[Trivia]) -> String {
    trivia
        .iter()
        .map(|trivia| trivia.lexeme.as_str())
        .collect::<String>()
}

fn command_operator_spacing_can_join_raw(operator: OperatorKind) -> bool {
    matches!(
        operator,
        OperatorKind::Equal
            | OperatorKind::NotEqual
            | OperatorKind::LessThan
            | OperatorKind::LessThanOrEqual
            | OperatorKind::GreaterThan
            | OperatorKind::GreaterThanOrEqual
            | OperatorKind::ShortCircuitAnd
            | OperatorKind::ShortCircuitOr
    )
}

fn command_argument_opens_group(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Delimiter(
            DelimiterKind::LeftParen | DelimiterKind::LeftBracket | DelimiterKind::LeftBrace
        )
    )
}

fn command_argument_closes_group(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Delimiter(
            DelimiterKind::RightParen | DelimiterKind::RightBracket | DelimiterKind::RightBrace
        )
    )
}

fn token_can_start_expression_kind(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Identifier
            | TokenKind::Keyword(Keyword::Methods)
            | TokenKind::Keyword(Keyword::Properties)
            | TokenKind::NumberLiteral(_)
            | TokenKind::CharLiteral
            | TokenKind::StringLiteral
            | TokenKind::Keyword(Keyword::End)
            | TokenKind::Delimiter(DelimiterKind::LeftParen)
            | TokenKind::Delimiter(DelimiterKind::LeftBracket)
            | TokenKind::Delimiter(DelimiterKind::LeftBrace)
            | TokenKind::Operator(OperatorKind::FunctionHandle)
            | TokenKind::Operator(OperatorKind::Plus)
            | TokenKind::Operator(OperatorKind::Minus)
            | TokenKind::Operator(OperatorKind::LogicalNot)
    )
}

fn token_can_start_command_argument(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Identifier
            | TokenKind::Keyword(_)
            | TokenKind::NumberLiteral(_)
            | TokenKind::CharLiteral
            | TokenKind::StringLiteral
    )
}

fn token_can_continue_command_argument(kind: &TokenKind) -> bool {
    token_can_start_command_argument(kind)
        || matches!(
            kind,
            TokenKind::Operator(
                OperatorKind::Dot
                    | OperatorKind::Colon
                    | OperatorKind::Minus
                    | OperatorKind::Plus
                    | OperatorKind::Multiply
                    | OperatorKind::RightDivide
                    | OperatorKind::LeftDivide
                    | OperatorKind::Power
                    | OperatorKind::ElementwiseMultiply
                    | OperatorKind::ElementwiseRightDivide
                    | OperatorKind::ElementwiseLeftDivide
                    | OperatorKind::ElementwisePower
                    | OperatorKind::LessThan
                    | OperatorKind::LessThanOrEqual
                    | OperatorKind::GreaterThan
                    | OperatorKind::GreaterThanOrEqual
                    | OperatorKind::Equal
                    | OperatorKind::NotEqual
                    | OperatorKind::LogicalAnd
                    | OperatorKind::LogicalOr
                    | OperatorKind::LogicalNot
                    | OperatorKind::ShortCircuitAnd
                    | OperatorKind::ShortCircuitOr
                    | OperatorKind::Assign
                    | OperatorKind::FunctionHandle
            ) | TokenKind::Delimiter(
                DelimiterKind::LeftParen
                    | DelimiterKind::RightParen
                    | DelimiterKind::LeftBracket
                    | DelimiterKind::RightBracket
                    | DelimiterKind::LeftBrace
                    | DelimiterKind::RightBrace
            )
        )
}

fn expression_as_qualified_name(expression: &Expression) -> Option<QualifiedName> {
    match &expression.kind {
        ExpressionKind::Identifier(identifier) => Some(QualifiedName {
            segments: vec![identifier.clone()],
            span: identifier.span,
        }),
        ExpressionKind::FieldAccess { target, field } => {
            let mut qualified = expression_as_qualified_name(target)?;
            qualified.segments.push(field.clone());
            qualified.span = expression.span;
            Some(qualified)
        }
        _ => None,
    }
}

fn function_handle_target_span(target: &FunctionHandleTarget) -> SourceSpan {
    match target {
        FunctionHandleTarget::Name(name) => name.span,
        FunctionHandleTarget::Expression(expression) => expression.span,
    }
}

impl Parser<'_> {
    fn function_handle_index_continues_receiver_chain(
        &self,
        open: DelimiterKind,
        close: DelimiterKind,
    ) -> bool {
        if !self.at_delimiter(open) {
            return false;
        }

        let mut cursor = self.cursor;
        let mut current_open = open;
        let mut current_close = close;

        loop {
            let mut depth = 0usize;
            let mut index = cursor;
            let Some(token) = self.tokens.get(index) else {
                return false;
            };
            if token.kind != TokenKind::Delimiter(current_open) {
                return false;
            }

            while let Some(token) = self.tokens.get(index) {
                match token.kind {
                    TokenKind::Delimiter(kind) if kind == current_open => depth += 1,
                    TokenKind::Delimiter(kind) if kind == current_close => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            match self.tokens.get(index + 1).map(|next| &next.kind) {
                                Some(TokenKind::Operator(OperatorKind::Dot)) => return true,
                                Some(TokenKind::Delimiter(DelimiterKind::LeftParen)) => {
                                    cursor = index + 1;
                                    current_open = DelimiterKind::LeftParen;
                                    current_close = DelimiterKind::RightParen;
                                    break;
                                }
                                Some(TokenKind::Delimiter(DelimiterKind::LeftBrace)) => {
                                    cursor = index + 1;
                                    current_open = DelimiterKind::LeftBrace;
                                    current_close = DelimiterKind::RightBrace;
                                    break;
                                }
                                _ => return false,
                            }
                        }
                    }
                    TokenKind::EndOfFile => return false,
                    _ => {}
                }
                index += 1;
            }

            if index >= self.tokens.len() {
                return false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_source, ParseMode};
    use crate::{
        ast::{
            AssignmentTarget, BinaryOp, ClassMemberAccess, CompilationUnitKind, ExpressionKind,
            FunctionHandleTarget, IndexArgument, Item, StatementKind,
        },
        source::SourceFileId,
    };

    #[test]
    fn parses_basic_script_assignment() {
        let parsed = parse_source("x = 1 + 2\n", SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");
        assert_eq!(unit.kind, CompilationUnitKind::Script);
        let Item::Statement(statement) = &unit.items[0] else {
            panic!("expected statement");
        };
        let StatementKind::Assignment { targets, value, .. } = &statement.kind else {
            panic!("expected assignment");
        };
        assert_eq!(targets.len(), 1);
        assert!(matches!(value.kind, ExpressionKind::Binary { .. }));
    }

    #[test]
    fn parses_function_header_and_body() {
        let source = "function y = add1(x)\ny = x + 1;\nend\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::AutoDetect);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");
        assert_eq!(unit.kind, CompilationUnitKind::FunctionFile);
        let Item::Function(function) = &unit.items[0] else {
            panic!("expected function");
        };
        assert_eq!(function.name.name, "add1");
        assert_eq!(function.inputs.len(), 1);
        assert_eq!(function.outputs.len(), 1);
        assert_eq!(function.body.len(), 1);
    }

    #[test]
    fn parses_multi_assignment() {
        let parsed = parse_source("[a, b] = deal(x)\n", SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");
        let Item::Statement(statement) = &unit.items[0] else {
            panic!("expected statement");
        };
        match &statement.kind {
            StatementKind::Assignment {
                targets,
                list_assignment,
                ..
            } => {
                assert_eq!(targets.len(), 2);
                assert!(*list_assignment);
            }
            _ => panic!("expected assignment"),
        }
    }

    #[test]
    fn distinguishes_bracketed_single_target_assignment_from_plain_assignment() {
        let bracketed = parse_source("[s.field] = [1 2]\n", SourceFileId(1), ParseMode::Script);
        assert!(!bracketed.has_errors(), "{:?}", bracketed.diagnostics);
        let bracketed_unit = bracketed.unit.expect("compilation unit");
        let Item::Statement(bracketed_statement) = &bracketed_unit.items[0] else {
            panic!("expected statement");
        };
        let StatementKind::Assignment {
            list_assignment, ..
        } = &bracketed_statement.kind
        else {
            panic!("expected assignment");
        };
        assert!(*list_assignment);

        let plain = parse_source("s.field = [1 2]\n", SourceFileId(1), ParseMode::Script);
        assert!(!plain.has_errors(), "{:?}", plain.diagnostics);
        let plain_unit = plain.unit.expect("compilation unit");
        let Item::Statement(plain_statement) = &plain_unit.items[0] else {
            panic!("expected statement");
        };
        let StatementKind::Assignment {
            list_assignment, ..
        } = &plain_statement.kind
        else {
            panic!("expected assignment");
        };
        assert!(!*list_assignment);
    }

    #[test]
    fn parses_multi_assignment_with_index_cell_and_field_targets() {
        let parsed = parse_source(
            "[vec(1), grid{1, 2}, s.value] = deal(x)\n",
            SourceFileId(1),
            ParseMode::Script,
        );
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");
        let Item::Statement(statement) = &unit.items[0] else {
            panic!("expected statement");
        };
        let StatementKind::Assignment { targets, .. } = &statement.kind else {
            panic!("expected assignment");
        };
        assert!(matches!(targets[0], AssignmentTarget::Index { .. }));
        assert!(matches!(targets[1], AssignmentTarget::CellIndex { .. }));
        assert!(matches!(targets[2], AssignmentTarget::Field { .. }));
    }

    #[test]
    fn parses_local_function_inside_function_body() {
        let source = "function y = outer(x)\ny = x;\nfunction z = inner(v)\nz = v + x;\nend\nend\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::AutoDetect);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");
        let Item::Function(function) = &unit.items[0] else {
            panic!("expected function");
        };
        assert_eq!(function.body.len(), 1);
        assert_eq!(function.local_functions.len(), 1);
        assert_eq!(function.local_functions[0].name.name, "inner");
    }

    #[test]
    fn parses_if_elseif_else_block() {
        let source = "if x < 0\nx = 1;\nelseif x < 1\nx = 2;\nelse\nx = 3;\nend\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");
        let Item::Statement(statement) = &unit.items[0] else {
            panic!("expected statement");
        };
        match &statement.kind {
            StatementKind::If {
                branches,
                else_body,
            } => {
                assert_eq!(branches.len(), 2);
                assert_eq!(branches[0].body.len(), 1);
                assert_eq!(branches[1].body.len(), 1);
                assert_eq!(else_body.len(), 1);
            }
            _ => panic!("expected if statement"),
        }
    }

    #[test]
    fn parses_switch_case_otherwise_block() {
        let source = "switch mode\ncase 0\nx = 1;\ncase {1, 2}\nx = 2;\notherwise\nx = 3;\nend\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");
        let Item::Statement(statement) = &unit.items[0] else {
            panic!("expected statement");
        };
        match &statement.kind {
            StatementKind::Switch {
                expression,
                cases,
                otherwise_body,
            } => {
                assert!(matches!(expression.kind, ExpressionKind::Identifier(_)));
                assert_eq!(cases.len(), 2);
                assert_eq!(cases[0].body.len(), 1);
                assert_eq!(cases[1].body.len(), 1);
                assert_eq!(otherwise_body.len(), 1);
            }
            _ => panic!("expected switch statement"),
        }
    }

    #[test]
    fn parses_try_catch_block() {
        let source = "try\nx = risky(1);\ncatch err\nx = err.message;\nend\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");
        let Item::Statement(statement) = &unit.items[0] else {
            panic!("expected statement");
        };
        match &statement.kind {
            StatementKind::Try {
                body,
                catch_binding,
                catch_body,
            } => {
                assert_eq!(body.len(), 1);
                assert_eq!(
                    catch_binding.as_ref().map(|binding| binding.name.as_str()),
                    Some("err")
                );
                assert_eq!(catch_body.len(), 1);
            }
            _ => panic!("expected try statement"),
        }
    }

    #[test]
    fn parses_package_qualified_function_references() {
        let source = "f = @pkg.helper;\ny = pkg.helper(1);\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let Item::Statement(first) = &unit.items[0] else {
            panic!("expected first statement");
        };
        let StatementKind::Assignment { value, .. } = &first.kind else {
            panic!("expected first assignment");
        };
        let ExpressionKind::FunctionHandle(FunctionHandleTarget::Name(name)) = &value.kind else {
            panic!("expected function handle");
        };
        assert_eq!(
            name.segments
                .iter()
                .map(|segment| segment.name.as_str())
                .collect::<Vec<_>>(),
            vec!["pkg", "helper"]
        );

        let Item::Statement(second) = &unit.items[1] else {
            panic!("expected second statement");
        };
        let StatementKind::Assignment { value, .. } = &second.kind else {
            panic!("expected second assignment");
        };
        let ExpressionKind::ParenApply { target, .. } = &value.kind else {
            panic!("expected apply expression");
        };
        let ExpressionKind::FieldAccess { target, field } = &target.kind else {
            panic!("expected field access");
        };
        let ExpressionKind::Identifier(root) = &target.kind else {
            panic!("expected root identifier");
        };
        assert_eq!(root.name, "pkg");
        assert_eq!(field.name, "helper");
    }

    #[test]
    fn parses_indexed_receiver_function_handle_targets() {
        let source = "f = @objs(:,2).total;\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let Item::Statement(statement) = &unit.items[0] else {
            panic!("expected statement");
        };
        let StatementKind::Assignment { value, .. } = &statement.kind else {
            panic!("expected assignment");
        };
        let ExpressionKind::FunctionHandle(FunctionHandleTarget::Expression(target)) = &value.kind
        else {
            panic!("expected expression-backed function handle");
        };
        let ExpressionKind::FieldAccess { target, field } = &target.kind else {
            panic!("expected field access");
        };
        let ExpressionKind::ParenApply { target, indices } = &target.kind else {
            panic!("expected indexed receiver");
        };
        let ExpressionKind::Identifier(root) = &target.kind else {
            panic!("expected root identifier");
        };
        assert_eq!(root.name, "objs");
        assert_eq!(field.name, "total");
        assert_eq!(indices.len(), 2);
        assert!(matches!(indices[0], IndexArgument::FullSlice));
    }

    #[test]
    fn parses_method_produced_indexed_receiver_function_handle_targets() {
        let source = "f = @objs.duplicate()(3).total;\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let Item::Statement(statement) = &unit.items[0] else {
            panic!("expected statement");
        };
        let StatementKind::Assignment { value, .. } = &statement.kind else {
            panic!("expected assignment");
        };
        let ExpressionKind::FunctionHandle(FunctionHandleTarget::Expression(target)) = &value.kind
        else {
            panic!("expected expression-backed function handle");
        };
        let ExpressionKind::FieldAccess { target, field } = &target.kind else {
            panic!("expected field access");
        };
        let ExpressionKind::ParenApply { target, indices } = &target.kind else {
            panic!("expected indexed receiver after method result");
        };
        let ExpressionKind::ParenApply {
            target,
            indices: first_indices,
        } = &target.kind
        else {
            panic!("expected method call receiver");
        };
        let ExpressionKind::FieldAccess {
            target,
            field: method,
        } = &target.kind
        else {
            panic!("expected method field access");
        };
        let ExpressionKind::Identifier(root) = &target.kind else {
            panic!("expected root identifier");
        };
        assert_eq!(root.name, "objs");
        assert_eq!(method.name, "duplicate");
        assert_eq!(field.name, "total");
        assert_eq!(first_indices.len(), 0);
        assert_eq!(indices.len(), 1);
    }

    #[test]
    fn parses_whitespace_separated_matrix_and_cell_rows() {
        let source = "x = [a b; c d];\ncells = {1 2; 3 4};\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let Item::Statement(first) = &unit.items[0] else {
            panic!("expected first statement");
        };
        let StatementKind::Assignment { value, .. } = &first.kind else {
            panic!("expected first assignment");
        };
        let ExpressionKind::MatrixLiteral(rows) = &value.kind else {
            panic!("expected matrix literal");
        };
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].len(), 2);
        assert_eq!(rows[1].len(), 2);

        let Item::Statement(second) = &unit.items[1] else {
            panic!("expected second statement");
        };
        let StatementKind::Assignment { value, .. } = &second.kind else {
            panic!("expected second assignment");
        };
        let ExpressionKind::CellLiteral(rows) = &value.kind else {
            panic!("expected cell literal");
        };
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].len(), 2);
        assert_eq!(rows[1].len(), 2);
    }

    #[test]
    fn parses_for_block() {
        let source = "for i = 1:10\nx = i;\nend\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");
        let Item::Statement(statement) = &unit.items[0] else {
            panic!("expected statement");
        };
        match &statement.kind {
            StatementKind::For { variable, body, .. } => {
                assert_eq!(variable.name, "i");
                assert_eq!(body.len(), 1);
            }
            _ => panic!("expected for statement"),
        }
    }

    #[test]
    fn parses_while_block() {
        let source = "while x < 10\nx = x + 1;\nend\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");
        let Item::Statement(statement) = &unit.items[0] else {
            panic!("expected statement");
        };
        match &statement.kind {
            StatementKind::While { body, .. } => assert_eq!(body.len(), 1),
            _ => panic!("expected while statement"),
        }
    }

    #[test]
    fn parses_nested_control_flow_inside_function() {
        let source = "function y = clamp1(x)\nif x < 1\ny = 1;\nelse\ny = x;\nend\nend\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::AutoDetect);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");
        let Item::Function(function) = &unit.items[0] else {
            panic!("expected function");
        };
        assert_eq!(function.body.len(), 1);
        match &function.body[0].kind {
            StatementKind::If {
                branches,
                else_body,
            } => {
                assert_eq!(branches.len(), 1);
                assert_eq!(else_body.len(), 1);
            }
            _ => panic!("expected nested if statement"),
        }
    }

    #[test]
    fn parses_command_form_expression_and_assignment_rhs() {
        let source =
            "string pkg.sub folder/file.txt name-value 1:3 -flag ./tmp/file @helper ../rel\nx = strcmp pkg.sub pkg.sub\ny = strcmp alpha, alpha\nw = pkg.helper alpha\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let Item::Statement(first) = &unit.items[0] else {
            panic!("expected first statement");
        };
        let StatementKind::Expression(expression) = &first.kind else {
            panic!("expected command-form expression");
        };
        let ExpressionKind::ParenApply { target, indices } = &expression.kind else {
            panic!("expected command-form apply");
        };
        assert!(matches!(target.kind, ExpressionKind::Identifier(_)));
        assert_eq!(indices.len(), 8);
        let IndexArgument::Expression(first) = &indices[0] else {
            panic!("expected first command argument");
        };
        assert!(matches!(
            &first.kind,
            ExpressionKind::CharLiteral(text) if text == "'pkg.sub'"
        ));
        let IndexArgument::Expression(second) = &indices[1] else {
            panic!("expected second command argument");
        };
        assert!(matches!(
            &second.kind,
            ExpressionKind::CharLiteral(text) if text == "'folder/file.txt'"
        ));
        let IndexArgument::Expression(third) = &indices[2] else {
            panic!("expected third command argument");
        };
        assert!(matches!(
            &third.kind,
            ExpressionKind::CharLiteral(text) if text == "'name-value'"
        ));
        let IndexArgument::Expression(fourth) = &indices[3] else {
            panic!("expected fourth command argument");
        };
        assert!(matches!(
            &fourth.kind,
            ExpressionKind::CharLiteral(text) if text == "'1:3'"
        ));
        let IndexArgument::Expression(fifth) = &indices[4] else {
            panic!("expected fifth command argument");
        };
        assert!(matches!(
            &fifth.kind,
            ExpressionKind::CharLiteral(text) if text == "'-flag'"
        ));
        let IndexArgument::Expression(sixth) = &indices[5] else {
            panic!("expected sixth command argument");
        };
        assert!(matches!(
            &sixth.kind,
            ExpressionKind::CharLiteral(text) if text == "'./tmp/file'"
        ));
        let IndexArgument::Expression(seventh) = &indices[6] else {
            panic!("expected seventh command argument");
        };
        assert!(matches!(
            &seventh.kind,
            ExpressionKind::CharLiteral(text) if text == "'@helper'"
        ));
        let IndexArgument::Expression(eighth) = &indices[7] else {
            panic!("expected eighth command argument");
        };
        assert!(matches!(
            &eighth.kind,
            ExpressionKind::CharLiteral(text) if text == "'../rel'"
        ));

        let Item::Statement(second) = &unit.items[1] else {
            panic!("expected second statement");
        };
        let StatementKind::Assignment { value, .. } = &second.kind else {
            panic!("expected assignment");
        };
        let ExpressionKind::ParenApply { indices, .. } = &value.kind else {
            panic!("expected command-form rhs");
        };
        assert_eq!(indices.len(), 2);
        let IndexArgument::Expression(first) = &indices[0] else {
            panic!("expected command rhs arg");
        };
        assert!(matches!(
            &first.kind,
            ExpressionKind::CharLiteral(text) if text == "'pkg.sub'"
        ));

        let Item::Statement(third) = &unit.items[2] else {
            panic!("expected third statement");
        };
        let StatementKind::Assignment { value, .. } = &third.kind else {
            panic!("expected comma-separated command assignment");
        };
        let ExpressionKind::ParenApply { indices, .. } = &value.kind else {
            panic!("expected comma-separated command-form rhs");
        };
        assert_eq!(indices.len(), 2);
        let IndexArgument::Expression(second) = &indices[1] else {
            panic!("expected second comma-separated command rhs arg");
        };
        assert!(matches!(
            &second.kind,
            ExpressionKind::CharLiteral(text) if text == "'alpha'"
        ));

        let Item::Statement(fourth) = &unit.items[3] else {
            panic!("expected fourth statement");
        };
        let StatementKind::Assignment { value, .. } = &fourth.kind else {
            panic!("expected dotted-target command assignment");
        };
        let ExpressionKind::ParenApply { target, indices } = &value.kind else {
            panic!("expected dotted-target command-form rhs");
        };
        let ExpressionKind::FieldAccess { target, field } = &target.kind else {
            panic!("expected dotted command target field access");
        };
        let ExpressionKind::Identifier(root) = &target.kind else {
            panic!("expected dotted command target root identifier");
        };
        assert_eq!(root.name, "pkg");
        assert_eq!(field.name, "helper");
        let IndexArgument::Expression(first) = &indices[0] else {
            panic!("expected dotted command arg");
        };
        assert!(matches!(
            &first.kind,
            ExpressionKind::CharLiteral(text) if text == "'alpha'"
        ));
    }

    #[test]
    fn parses_methods_and_properties_keywords_as_callable_identifiers_in_expressions() {
        let source = "x = methods(obj);\ny = properties(obj);\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let expected = ["methods", "properties"];
        for (item, expected_name) in unit.items.iter().zip(expected) {
            let Item::Statement(statement) = item else {
                panic!("expected statement");
            };
            let StatementKind::Assignment { value, .. } = &statement.kind else {
                panic!("expected assignment");
            };
            let ExpressionKind::ParenApply { target, .. } = &value.kind else {
                panic!("expected call expression");
            };
            let ExpressionKind::Identifier(identifier) = &target.kind else {
                panic!("expected identifier target");
            };
            assert_eq!(identifier.name, expected_name);
        }
    }

    #[test]
    fn parses_methods_and_properties_keywords_in_command_form() {
        let source = "methods Child\nx = properties Child\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let Item::Statement(first) = &unit.items[0] else {
            panic!("expected first statement");
        };
        let StatementKind::Expression(expression) = &first.kind else {
            panic!("expected command-form expression");
        };
        let ExpressionKind::ParenApply { target, indices } = &expression.kind else {
            panic!("expected command-form apply");
        };
        let ExpressionKind::Identifier(identifier) = &target.kind else {
            panic!("expected identifier target");
        };
        assert_eq!(identifier.name, "methods");
        assert_eq!(indices.len(), 1);

        let Item::Statement(second) = &unit.items[1] else {
            panic!("expected second statement");
        };
        let StatementKind::Assignment { value, .. } = &second.kind else {
            panic!("expected assignment");
        };
        let ExpressionKind::ParenApply { target, indices } = &value.kind else {
            panic!("expected command-form rhs");
        };
        let ExpressionKind::Identifier(identifier) = &target.kind else {
            panic!("expected identifier target");
        };
        assert_eq!(identifier.name, "properties");
        assert_eq!(indices.len(), 1);
    }

    #[test]
    fn parses_command_form_raw_operator_runs_and_embedded_quotes() {
        let source = "x = cmdpkg.helper name=value\ny = cmdpkg.helper alpha==beta\nz = cmdpkg.helper lower<=upper\na = cmdpkg.helper left~=right\nb = cmdpkg.helper opt&&flag\nc = cmdpkg.helper opt||flag\nd = cmdpkg.helper key=\"two words\"\ne = cmdpkg.helper key='two words'\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let expected = [
            "'name=value'",
            "'alpha==beta'",
            "'lower<=upper'",
            "'left~=right'",
            "'opt&&flag'",
            "'opt||flag'",
            "'key=\"two words\"'",
            "'key=''two words'''",
        ];

        assert_eq!(unit.items.len(), expected.len());
        for (item, expected_argument) in unit.items.iter().zip(expected) {
            let Item::Statement(statement) = item else {
                panic!("expected statement");
            };
            let StatementKind::Assignment { value, .. } = &statement.kind else {
                panic!("expected assignment");
            };
            let ExpressionKind::ParenApply { indices, .. } = &value.kind else {
                panic!("expected command-form rhs");
            };
            assert_eq!(indices.len(), 1);
            let IndexArgument::Expression(argument) = &indices[0] else {
                panic!("expected command argument");
            };
            assert!(matches!(
                &argument.kind,
                ExpressionKind::CharLiteral(text) if text == expected_argument
            ));
        }
    }

    #[test]
    fn line_continuation_only_does_not_force_command_argument_boundaries() {
        let source = "x = cmdpkg.helper name=... comment\nvalue\ny = cmdpkg.helper alpha...\nbeta\nz = cmdpkg.helper alpha ... comment\nbeta\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let expected = [
            vec!["'name=value'"],
            vec!["'alphabeta'"],
            vec!["'alpha'", "'beta'"],
        ];

        assert_eq!(unit.items.len(), expected.len());
        for (item, expected_arguments) in unit.items.iter().zip(expected) {
            let Item::Statement(statement) = item else {
                panic!("expected statement");
            };
            let StatementKind::Assignment { value, .. } = &statement.kind else {
                panic!("expected assignment");
            };
            let ExpressionKind::ParenApply { indices, .. } = &value.kind else {
                panic!("expected command-form rhs");
            };
            assert_eq!(indices.len(), expected_arguments.len());
            for (argument, expected_text) in indices.iter().zip(expected_arguments) {
                let IndexArgument::Expression(argument) = argument else {
                    panic!("expected command argument");
                };
                assert!(matches!(
                    &argument.kind,
                    ExpressionKind::CharLiteral(text) if text == expected_text
                ));
            }
        }
    }

    #[test]
    fn assignment_rhs_prefers_expression_for_bare_signed_numbers() {
        let source =
            "base = 5\nx = base -1\ny = base +1\nz = cmdpkg.helper -1:3\na = cmdpkg.helper +1ms\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let Item::Statement(second) = &unit.items[1] else {
            panic!("expected second statement");
        };
        let StatementKind::Assignment { value, .. } = &second.kind else {
            panic!("expected assignment");
        };
        assert!(matches!(
            &value.kind,
            ExpressionKind::Binary {
                op: BinaryOp::Subtract,
                ..
            }
        ));

        let Item::Statement(third) = &unit.items[2] else {
            panic!("expected third statement");
        };
        let StatementKind::Assignment { value, .. } = &third.kind else {
            panic!("expected assignment");
        };
        assert!(matches!(
            &value.kind,
            ExpressionKind::Binary {
                op: BinaryOp::Add,
                ..
            }
        ));

        let expected = ["'-1:3'", "'+1ms'"];
        for (item, expected_argument) in unit.items[3..].iter().zip(expected) {
            let Item::Statement(statement) = item else {
                panic!("expected statement");
            };
            let StatementKind::Assignment { value, .. } = &statement.kind else {
                panic!("expected assignment");
            };
            let ExpressionKind::ParenApply { indices, .. } = &value.kind else {
                panic!("expected command-form rhs");
            };
            assert_eq!(indices.len(), 1);
            let IndexArgument::Expression(argument) = &indices[0] else {
                panic!("expected command argument");
            };
            assert!(matches!(
                &argument.kind,
                ExpressionKind::CharLiteral(text) if text == expected_argument
            ));
        }
    }

    #[test]
    fn assignment_rhs_prefers_expression_for_short_elementwise_division() {
        let source =
            "div_base = 8\ndivisor = 2\nx = div_base ./divisor\ny = div_base .\\divisor\nz = cmdpkg.helper ./tmp/file\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let Item::Statement(third) = &unit.items[2] else {
            panic!("expected third statement");
        };
        let StatementKind::Assignment { value, .. } = &third.kind else {
            panic!("expected assignment");
        };
        assert!(matches!(
            &value.kind,
            ExpressionKind::Binary {
                op: BinaryOp::ElementwiseRightDivide,
                ..
            }
        ));

        let Item::Statement(fourth) = &unit.items[3] else {
            panic!("expected fourth statement");
        };
        let StatementKind::Assignment { value, .. } = &fourth.kind else {
            panic!("expected assignment");
        };
        assert!(matches!(
            &value.kind,
            ExpressionKind::Binary {
                op: BinaryOp::ElementwiseLeftDivide,
                ..
            }
        ));

        let Item::Statement(fifth) = &unit.items[4] else {
            panic!("expected fifth statement");
        };
        let StatementKind::Assignment { value, .. } = &fifth.kind else {
            panic!("expected assignment");
        };
        let ExpressionKind::ParenApply { indices, .. } = &value.kind else {
            panic!("expected command-form rhs");
        };
        assert_eq!(indices.len(), 1);
        let IndexArgument::Expression(argument) = &indices[0] else {
            panic!("expected command argument");
        };
        assert!(matches!(
            &argument.kind,
            ExpressionKind::CharLiteral(text) if text == "'./tmp/file'"
        ));
    }

    #[test]
    fn assignment_rhs_prefers_expression_for_dotted_short_division() {
        let source = "rhs = 2\nx = dotted_base.helper /rhs\ny = dotted_base.helper \\rhs\nz = dotted_base.helper ./rhs\na = dotted_base.helper .\\rhs\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let expected_ops = [
            BinaryOp::MatrixRightDivide,
            BinaryOp::MatrixLeftDivide,
            BinaryOp::ElementwiseRightDivide,
            BinaryOp::ElementwiseLeftDivide,
        ];
        for (item, expected_op) in unit.items[1..].iter().zip(expected_ops) {
            let Item::Statement(statement) = item else {
                panic!("expected statement");
            };
            let StatementKind::Assignment { value, .. } = &statement.kind else {
                panic!("expected assignment");
            };
            assert!(matches!(
                &value.kind,
                ExpressionKind::Binary { op, .. } if *op == expected_op
            ));
        }
    }

    #[test]
    fn assignment_rhs_prefers_expression_for_short_matrix_division() {
        let source =
            "div_base = 8\ndivisor = 2\nx = div_base /divisor\ny = div_base \\divisor\nz = cmdpkg.helper /tmp/file\na = cmdpkg.helper \\tmp\\file\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let Item::Statement(third) = &unit.items[2] else {
            panic!("expected third statement");
        };
        let StatementKind::Assignment { value, .. } = &third.kind else {
            panic!("expected assignment");
        };
        assert!(matches!(
            &value.kind,
            ExpressionKind::Binary {
                op: BinaryOp::MatrixRightDivide,
                ..
            }
        ));

        let Item::Statement(fourth) = &unit.items[3] else {
            panic!("expected fourth statement");
        };
        let StatementKind::Assignment { value, .. } = &fourth.kind else {
            panic!("expected assignment");
        };
        assert!(matches!(
            &value.kind,
            ExpressionKind::Binary {
                op: BinaryOp::MatrixLeftDivide,
                ..
            }
        ));

        let expected = ["'/tmp/file'", "'\\tmp\\file'"];
        for (item, expected_argument) in unit.items[4..].iter().zip(expected) {
            let Item::Statement(statement) = item else {
                panic!("expected statement");
            };
            let StatementKind::Assignment { value, .. } = &statement.kind else {
                panic!("expected assignment");
            };
            let ExpressionKind::ParenApply { indices, .. } = &value.kind else {
                panic!("expected command-form rhs");
            };
            assert_eq!(indices.len(), 1);
            let IndexArgument::Expression(argument) = &indices[0] else {
                panic!("expected command argument");
            };
            assert!(matches!(
                &argument.kind,
                ExpressionKind::CharLiteral(text) if text == expected_argument
            ));
        }
    }

    #[test]
    fn assignment_rhs_prefers_expression_for_short_multiply_and_power() {
        let source =
            "mul_base = 6\nmul_rhs = 2\nx = mul_base *mul_rhs\ny = mul_base .*mul_rhs\nz = mul_base ^mul_rhs\na = mul_base .^mul_rhs\nb = cmdpkg.helper *.txt\nc = cmdpkg.helper .*glob\nd = cmdpkg.helper ^caret\ne = cmdpkg.helper .^caret\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let expected_ops = [
            BinaryOp::Multiply,
            BinaryOp::ElementwiseMultiply,
            BinaryOp::Power,
            BinaryOp::ElementwisePower,
        ];
        for (item, expected_op) in unit.items[2..6].iter().zip(expected_ops) {
            let Item::Statement(statement) = item else {
                panic!("expected statement");
            };
            let StatementKind::Assignment { value, .. } = &statement.kind else {
                panic!("expected assignment");
            };
            assert!(matches!(
                &value.kind,
                ExpressionKind::Binary { op, .. } if *op == expected_op
            ));
        }

        let Item::Statement(seventh) = &unit.items[6] else {
            panic!("expected seventh statement");
        };
        let StatementKind::Assignment { value, .. } = &seventh.kind else {
            panic!("expected assignment");
        };
        let ExpressionKind::ParenApply { indices, .. } = &value.kind else {
            panic!("expected command-form rhs");
        };
        assert_eq!(indices.len(), 1);
        let IndexArgument::Expression(argument) = &indices[0] else {
            panic!("expected command argument");
        };
        assert!(matches!(
            &argument.kind,
            ExpressionKind::CharLiteral(text) if text == "'*.txt'"
        ));

        let expected_ops = [
            BinaryOp::ElementwiseMultiply,
            BinaryOp::Power,
            BinaryOp::ElementwisePower,
        ];
        for (item, expected_op) in unit.items[7..].iter().zip(expected_ops) {
            let Item::Statement(statement) = item else {
                panic!("expected statement");
            };
            let StatementKind::Assignment { value, .. } = &statement.kind else {
                panic!("expected assignment");
            };
            assert!(matches!(
                &value.kind,
                ExpressionKind::Binary { op, .. } if *op == expected_op
            ));
        }
    }

    #[test]
    fn assignment_rhs_prefers_expression_for_dotted_short_multiply_and_power() {
        let source = "rhs = 2\nx = dotted_base.helper *rhs\ny = dotted_base.helper .*rhs\nz = dotted_base.helper ^rhs\na = dotted_base.helper .^rhs\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let expected_ops = [
            BinaryOp::Multiply,
            BinaryOp::ElementwiseMultiply,
            BinaryOp::Power,
            BinaryOp::ElementwisePower,
        ];
        for (item, expected_op) in unit.items[1..].iter().zip(expected_ops) {
            let Item::Statement(statement) = item else {
                panic!("expected statement");
            };
            let StatementKind::Assignment { value, .. } = &statement.kind else {
                panic!("expected assignment");
            };
            assert!(matches!(
                &value.kind,
                ExpressionKind::Binary { op, .. } if *op == expected_op
            ));
        }
    }

    #[test]
    fn assignment_rhs_prefers_expression_for_short_operator_field_chains() {
        let source = "a = base /foo.bar\nb = base *foo.bar\nc = base ^foo.bar\nd = base ./foo.bar\ne = base .*foo.bar\nf = base .^foo.bar\ng = cmdpkg.helper /foo.bar\nh = cmdpkg.helper *foo.bar\ni = cmdpkg.helper ^foo.bar\nj = cmdpkg.helper ./foo.bar\nk = cmdpkg.helper .*foo.bar\nl = cmdpkg.helper .^foo.bar\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let expected_ops = [
            BinaryOp::MatrixRightDivide,
            BinaryOp::Multiply,
            BinaryOp::Power,
            BinaryOp::ElementwiseRightDivide,
            BinaryOp::ElementwiseMultiply,
            BinaryOp::ElementwisePower,
            BinaryOp::MatrixRightDivide,
            BinaryOp::Multiply,
            BinaryOp::Power,
            BinaryOp::ElementwiseRightDivide,
            BinaryOp::ElementwiseMultiply,
            BinaryOp::ElementwisePower,
        ];
        for (item, expected_op) in unit.items.iter().zip(expected_ops) {
            let Item::Statement(statement) = item else {
                panic!("expected statement");
            };
            let StatementKind::Assignment { value, .. } = &statement.kind else {
                panic!("expected assignment");
            };
            let ExpressionKind::Binary { op, rhs, .. } = &value.kind else {
                panic!("expected binary rhs");
            };
            assert_eq!(*op, expected_op);
            assert!(matches!(&rhs.kind, ExpressionKind::FieldAccess { .. }));
        }
    }

    #[test]
    fn assignment_rhs_prefers_expression_for_short_operator_paren_postfix() {
        let source = "a = base /foo(1)\nb = base *foo(1)\nc = base ^foo(1)\nd = cmdpkg.helper /foo(1)\ne = cmdpkg.helper *foo(1)\nf = cmdpkg.helper ^foo(1)\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let expected_ops = [
            BinaryOp::MatrixRightDivide,
            BinaryOp::Multiply,
            BinaryOp::Power,
            BinaryOp::MatrixRightDivide,
            BinaryOp::Multiply,
            BinaryOp::Power,
        ];
        for (item, expected_op) in unit.items.iter().zip(expected_ops) {
            let Item::Statement(statement) = item else {
                panic!("expected statement");
            };
            let StatementKind::Assignment { value, .. } = &statement.kind else {
                panic!("expected assignment");
            };
            let ExpressionKind::Binary { op, rhs, .. } = &value.kind else {
                panic!("expected binary rhs");
            };
            assert_eq!(*op, expected_op);
            assert!(matches!(&rhs.kind, ExpressionKind::ParenApply { .. }));
        }
    }

    #[test]
    fn assignment_rhs_prefers_expression_for_short_operator_brace_postfix() {
        let source = "a = base /foo{1}\nb = base *foo{1}\nc = base ^foo{1}\nd = cmdpkg.helper /foo{1}\ne = cmdpkg.helper *foo{1}\nf = cmdpkg.helper ^foo{1}\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let expected_ops = [
            BinaryOp::MatrixRightDivide,
            BinaryOp::Multiply,
            BinaryOp::Power,
            BinaryOp::MatrixRightDivide,
            BinaryOp::Multiply,
            BinaryOp::Power,
        ];
        for (item, expected_op) in unit.items.iter().zip(expected_ops) {
            let Item::Statement(statement) = item else {
                panic!("expected statement");
            };
            let StatementKind::Assignment { value, .. } = &statement.kind else {
                panic!("expected assignment");
            };
            let ExpressionKind::Binary { op, rhs, .. } = &value.kind else {
                panic!("expected binary rhs");
            };
            assert_eq!(*op, expected_op);
            assert!(matches!(&rhs.kind, ExpressionKind::CellIndex { .. }));
        }
    }

    #[test]
    fn parses_command_form_grouped_delimiters_with_inner_separators() {
        let source = "x = cmdpkg.helper value(1,2)\ny = cmdpkg.helper nested{1, 2}\nz = cmdpkg.helper cell(1,[2, 3])\na = cmdpkg.helper matrix([1,2; 3,4])\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let expected = [
            "'value(1,2)'",
            "'nested{1,2}'",
            "'cell(1,[2,3])'",
            "'matrix([1,2;3,4])'",
        ];

        assert_eq!(unit.items.len(), expected.len());
        for (item, expected_argument) in unit.items.iter().zip(expected) {
            let Item::Statement(statement) = item else {
                panic!("expected statement");
            };
            let StatementKind::Assignment { value, .. } = &statement.kind else {
                panic!("expected assignment");
            };
            let ExpressionKind::ParenApply { indices, .. } = &value.kind else {
                panic!("expected command-form rhs");
            };
            assert_eq!(indices.len(), 1);
            let IndexArgument::Expression(argument) = &indices[0] else {
                panic!("expected command argument");
            };
            assert!(matches!(
                &argument.kind,
                ExpressionKind::CharLiteral(text) if text == expected_argument
            ));
        }
    }

    #[test]
    fn parses_command_form_leading_quoted_fragments_with_raw_suffixes() {
        let source = "x = cmdpkg.helper \"two words\".txt\ny = cmdpkg.helper 'two words'.m\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let expected = ["'\"two words\".txt'", "'''two words''.m'"];
        assert_eq!(unit.items.len(), expected.len());
        for (item, expected_argument) in unit.items.iter().zip(expected) {
            let Item::Statement(statement) = item else {
                panic!("expected statement");
            };
            let StatementKind::Assignment { value, .. } = &statement.kind else {
                panic!("expected assignment");
            };
            let ExpressionKind::ParenApply { indices, .. } = &value.kind else {
                panic!("expected command-form rhs");
            };
            assert_eq!(indices.len(), 1);
            let IndexArgument::Expression(argument) = &indices[0] else {
                panic!("expected command argument");
            };
            assert!(matches!(
                &argument.kind,
                ExpressionKind::CharLiteral(text) if text == expected_argument
            ));
        }
    }

    #[test]
    fn parses_command_form_quoted_fragments_with_separator_suffixes() {
        let source = "x = cmdpkg.helper 'two words',suffix\ny = cmdpkg.helper key=\"a;b\";tail\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let expected = ["'''two words'',suffix'", "'key=\"a;b\";tail'"];
        assert_eq!(unit.items.len(), expected.len());
        for (item, expected_argument) in unit.items.iter().zip(expected) {
            let Item::Statement(statement) = item else {
                panic!("expected statement");
            };
            let StatementKind::Assignment { value, .. } = &statement.kind else {
                panic!("expected assignment");
            };
            let ExpressionKind::ParenApply { indices, .. } = &value.kind else {
                panic!("expected command-form rhs");
            };
            assert_eq!(indices.len(), 1);
            let IndexArgument::Expression(argument) = &indices[0] else {
                panic!("expected command argument");
            };
            assert!(matches!(
                &argument.kind,
                ExpressionKind::CharLiteral(text) if text == expected_argument
            ));
        }
    }

    #[test]
    fn parses_command_form_grouped_and_quoted_delimiters_as_single_argument() {
        let source = "x = cmdpkg.helper value(1,\"a,b\",3)\ny = cmdpkg.helper note='%literal comment text%'\nz = cmdpkg.helper prefix\"two words\"suffix\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let expected = [
            "'value(1,\"a,b\",3)'",
            "'note=''%literal comment text%'''",
            "'prefix\"two words\"suffix'",
        ];
        assert_eq!(unit.items.len(), expected.len());
        for (item, expected_argument) in unit.items.iter().zip(expected) {
            let Item::Statement(statement) = item else {
                panic!("expected statement");
            };
            let StatementKind::Assignment { value, .. } = &statement.kind else {
                panic!("expected assignment");
            };
            let ExpressionKind::ParenApply { indices, .. } = &value.kind else {
                panic!("expected command-form rhs");
            };
            assert_eq!(indices.len(), 1);
            let IndexArgument::Expression(argument) = &indices[0] else {
                panic!("expected command argument");
            };
            assert!(matches!(
                &argument.kind,
                ExpressionKind::CharLiteral(text) if text == expected_argument
            ));
        }
    }

    #[test]
    fn parses_command_form_standalone_dot_argument() {
        let source = "x = what .\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");
        let Item::Statement(statement) = &unit.items[0] else {
            panic!("expected statement");
        };
        let StatementKind::Assignment { value, .. } = &statement.kind else {
            panic!("expected assignment");
        };
        let ExpressionKind::ParenApply { target, indices } = &value.kind else {
            panic!("expected command-form rhs");
        };
        let ExpressionKind::Identifier(identifier) = &target.kind else {
            panic!("expected identifier target");
        };
        assert_eq!(identifier.name, "what");
        assert_eq!(indices.len(), 1);
        let IndexArgument::Expression(argument) = &indices[0] else {
            panic!("expected command argument");
        };
        assert!(matches!(
            &argument.kind,
            ExpressionKind::CharLiteral(text) if text == "'.'"
        ));
    }

    #[test]
    fn parses_standalone_quoted_command_arguments_as_raw_text() {
        let source = "x = cmdpkg.helper \"two words\"\ny = cmdpkg.helper \"two words\" \"three words\"\nz = cmdpkg.helper 'two words'\na = cmdpkg.helper \"a,b\" more\nb = cmdpkg.helper \"a;b\" tail\nc = cmdpkg.helper key=\"a\"\"b\"\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let expected = [
            vec!["'\"two words\"'"],
            vec!["'\"two words\"'", "'\"three words\"'"],
            vec!["'''two words'''"],
            vec!["'\"a,b\"'", "'more'"],
            vec!["'\"a;b\"'", "'tail'"],
            vec!["'key=\"a\"\"b\"'"],
        ];
        assert_eq!(unit.items.len(), expected.len());
        for (item, expected_arguments) in unit.items.iter().zip(expected) {
            let Item::Statement(statement) = item else {
                panic!("expected statement");
            };
            let StatementKind::Assignment { value, .. } = &statement.kind else {
                panic!("expected assignment");
            };
            let ExpressionKind::ParenApply { indices, .. } = &value.kind else {
                panic!("expected command-form rhs");
            };
            assert_eq!(indices.len(), expected_arguments.len());
            for (argument, expected_text) in indices.iter().zip(expected_arguments) {
                let IndexArgument::Expression(argument) = argument else {
                    panic!("expected command argument");
                };
                assert!(matches!(
                    &argument.kind,
                    ExpressionKind::CharLiteral(text) if text == expected_text
                ));
            }
        }
    }

    #[test]
    fn parses_spaced_operator_command_arguments_as_raw_text() {
        let source = "x = cmdpkg.helper alpha == beta\ny = cmdpkg.helper alpha ~= beta\nz = cmdpkg.helper alpha <= beta\na = cmdpkg.helper alpha >= beta\nb = cmdpkg.helper alpha < beta\nc = cmdpkg.helper alpha > beta\nd = cmdpkg.helper alpha && beta\ne = cmdpkg.helper alpha || beta\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let expected = [
            "'alpha == beta'",
            "'alpha ~= beta'",
            "'alpha <= beta'",
            "'alpha >= beta'",
            "'alpha < beta'",
            "'alpha > beta'",
            "'alpha && beta'",
            "'alpha || beta'",
        ];
        assert_eq!(unit.items.len(), expected.len());
        for (item, expected_argument) in unit.items.iter().zip(expected) {
            let Item::Statement(statement) = item else {
                panic!("expected statement");
            };
            let StatementKind::Assignment { value, .. } = &statement.kind else {
                panic!("expected assignment");
            };
            let ExpressionKind::ParenApply { indices, .. } = &value.kind else {
                panic!("expected command-form rhs");
            };
            assert_eq!(indices.len(), 1);
            let IndexArgument::Expression(argument) = &indices[0] else {
                panic!("expected command argument");
            };
            assert!(matches!(
                &argument.kind,
                ExpressionKind::CharLiteral(text) if text == expected_argument
            ));
        }
    }

    #[test]
    fn assignment_rhs_prefers_expression_for_signed_grouped_forms() {
        let source = "base = 5\nx = base -(1 + 2)\ny = base +[1, 2]\nz = cmdpkg.helper -(1 + 2)\na = cmdpkg.helper +[1, 2]\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let Item::Statement(second) = &unit.items[1] else {
            panic!("expected second statement");
        };
        let StatementKind::Assignment { value, .. } = &second.kind else {
            panic!("expected assignment");
        };
        assert!(matches!(
            &value.kind,
            ExpressionKind::Binary {
                op: BinaryOp::Subtract,
                ..
            }
        ));

        let Item::Statement(third) = &unit.items[2] else {
            panic!("expected third statement");
        };
        let StatementKind::Assignment { value, .. } = &third.kind else {
            panic!("expected assignment");
        };
        assert!(matches!(
            &value.kind,
            ExpressionKind::Binary {
                op: BinaryOp::Add,
                ..
            }
        ));

        let expected_ops = [BinaryOp::Subtract, BinaryOp::Add];
        for (item, expected_op) in unit.items[3..].iter().zip(expected_ops) {
            let Item::Statement(statement) = item else {
                panic!("expected statement");
            };
            let StatementKind::Assignment { value, .. } = &statement.kind else {
                panic!("expected assignment");
            };
            assert!(matches!(
                &value.kind,
                ExpressionKind::Binary { op, .. } if *op == expected_op
            ));
        }
    }

    #[test]
    fn assignment_rhs_prefers_expression_for_dotted_signed_grouped_forms() {
        let source = "x = dotted_base.helper -(1 + 2)\ny = dotted_base.helper +[1, 2]\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let expected_ops = [BinaryOp::Subtract, BinaryOp::Add];
        for (item, expected_op) in unit.items.iter().zip(expected_ops) {
            let Item::Statement(statement) = item else {
                panic!("expected statement");
            };
            let StatementKind::Assignment { value, .. } = &statement.kind else {
                panic!("expected assignment");
            };
            assert!(matches!(
                &value.kind,
                ExpressionKind::Binary { op, .. } if *op == expected_op
            ));
        }
    }

    #[test]
    fn assignment_rhs_prefers_expression_for_signed_postfix_forms() {
        let source = "base = 5\nx = base -sum([1, 2])\ny = base +sum([1, 2])\nz = cmdpkg.helper -sum([1, 2])\na = cmdpkg.helper +sum([1, 2])\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let Item::Statement(second) = &unit.items[1] else {
            panic!("expected second statement");
        };
        let StatementKind::Assignment { value, .. } = &second.kind else {
            panic!("expected assignment");
        };
        assert!(matches!(
            &value.kind,
            ExpressionKind::Binary {
                op: BinaryOp::Subtract,
                ..
            }
        ));

        let Item::Statement(third) = &unit.items[2] else {
            panic!("expected third statement");
        };
        let StatementKind::Assignment { value, .. } = &third.kind else {
            panic!("expected assignment");
        };
        assert!(matches!(
            &value.kind,
            ExpressionKind::Binary {
                op: BinaryOp::Add,
                ..
            }
        ));

        let expected_ops = [BinaryOp::Subtract, BinaryOp::Add];
        for (item, expected_op) in unit.items[3..].iter().zip(expected_ops) {
            let Item::Statement(statement) = item else {
                panic!("expected statement");
            };
            let StatementKind::Assignment { value, .. } = &statement.kind else {
                panic!("expected assignment");
            };
            assert!(matches!(
                &value.kind,
                ExpressionKind::Binary { op, .. } if *op == expected_op
            ));
        }
    }

    #[test]
    fn assignment_rhs_prefers_expression_for_dotted_signed_call_like_forms() {
        let source = "x = dotted_base.helper -sum([1, 2])\ny = dotted_base.helper +sum([1, 2])\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let expected_ops = [BinaryOp::Subtract, BinaryOp::Add];
        for (item, expected_op) in unit.items.iter().zip(expected_ops) {
            let Item::Statement(statement) = item else {
                panic!("expected statement");
            };
            let StatementKind::Assignment { value, .. } = &statement.kind else {
                panic!("expected assignment");
            };
            assert!(matches!(
                &value.kind,
                ExpressionKind::Binary { op, .. } if *op == expected_op
            ));
        }
    }

    #[test]
    fn assignment_rhs_prefers_expression_for_signed_field_chains() {
        let source = "base = 5\nx = base -rhs.value\ny = base +rhs.items(2)\nz = cmdpkg.helper -rhs.value\na = cmdpkg.helper +rhs.items(2)\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let Item::Statement(second) = &unit.items[1] else {
            panic!("expected second statement");
        };
        let StatementKind::Assignment { value, .. } = &second.kind else {
            panic!("expected assignment");
        };
        assert!(matches!(
            &value.kind,
            ExpressionKind::Binary {
                op: BinaryOp::Subtract,
                ..
            }
        ));

        let Item::Statement(third) = &unit.items[2] else {
            panic!("expected third statement");
        };
        let StatementKind::Assignment { value, .. } = &third.kind else {
            panic!("expected assignment");
        };
        assert!(matches!(
            &value.kind,
            ExpressionKind::Binary {
                op: BinaryOp::Add,
                ..
            }
        ));

        let expected_ops = [BinaryOp::Subtract, BinaryOp::Add];
        for (item, expected_op) in unit.items[3..].iter().zip(expected_ops) {
            let Item::Statement(statement) = item else {
                panic!("expected statement");
            };
            let StatementKind::Assignment { value, .. } = &statement.kind else {
                panic!("expected assignment");
            };
            assert!(matches!(
                &value.kind,
                ExpressionKind::Binary { op, .. } if *op == expected_op
            ));
        }
    }

    #[test]
    fn assignment_rhs_prefers_expression_for_dotted_signed_field_chains() {
        let source = "x = dotted_base.helper -rhs.value\ny = dotted_base.helper +rhs.items(2)\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let expected_ops = [BinaryOp::Subtract, BinaryOp::Add];
        for (item, expected_op) in unit.items.iter().zip(expected_ops) {
            let Item::Statement(statement) = item else {
                panic!("expected statement");
            };
            let StatementKind::Assignment { value, .. } = &statement.kind else {
                panic!("expected assignment");
            };
            assert!(matches!(
                &value.kind,
                ExpressionKind::Binary { op, .. } if *op == expected_op
            ));
        }
    }

    #[test]
    fn statement_display_suppression_tracks_semicolons() {
        let parsed = parse_source("a = 1;\n2 + 3\n", SourceFileId(1), ParseMode::Script);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let unit = parsed.unit.expect("compilation unit");

        let Item::Statement(first) = &unit.items[0] else {
            panic!("expected statement");
        };
        assert!(first.display_suppressed);

        let Item::Statement(second) = &unit.items[1] else {
            panic!("expected statement");
        };
        assert!(!second.display_suppressed);
    }

    #[test]
    fn classdef_top_level_attributes_emit_explicit_diagnostics() {
        let source = "classdef (Sealed, ConstructOnLoad=true) Point\nend\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::AutoDetect);
        assert!(parsed.has_errors(), "expected parse diagnostics");
        let messages = parsed
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.clone())
            .collect::<Vec<_>>();
        assert!(messages
            .iter()
            .any(|message| { message.contains("`classdef` attribute `Sealed` is not supported") }));
        assert!(messages.iter().any(|message| {
            message.contains("`classdef` attribute `ConstructOnLoad` is not supported")
        }));

        let unit = parsed.unit.expect("compilation unit");
        assert_eq!(unit.kind, CompilationUnitKind::ClassFile);
        let Item::Class(class_def) = &unit.items[0] else {
            panic!("expected class item");
        };
        assert_eq!(class_def.name.name, "Point");
    }

    #[test]
    fn classdef_top_level_attributes_do_not_break_supported_block_attributes() {
        let source = "classdef (Sealed) Point\n\
                      properties (Access=private)\n\
                      secret\n\
                      end\n\
                      methods (Static, Access=private)\n\
                      function out = hidden()\n\
                      out = 1;\n\
                      end\n\
                      end\n\
                      end\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::AutoDetect);
        assert!(parsed.has_errors(), "expected parse diagnostics");
        let unit = parsed.unit.expect("compilation unit");
        let Item::Class(class_def) = &unit.items[0] else {
            panic!("expected class item");
        };
        assert_eq!(class_def.property_blocks.len(), 1);
        assert_eq!(
            class_def.property_blocks[0].access,
            ClassMemberAccess::Private
        );
        assert_eq!(class_def.method_blocks.len(), 1);
        assert!(class_def.method_blocks[0].is_static);
        assert_eq!(
            class_def.method_blocks[0].access,
            ClassMemberAccess::Private
        );
    }

    #[test]
    fn property_block_attributes_emit_explicit_diagnostics_for_unsupported_names() {
        let source = "classdef Point\n\
                      properties (Constant, Access=private, Dependent=true)\n\
                      value\n\
                      end\n\
                      end\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::AutoDetect);
        assert!(parsed.has_errors(), "expected parse diagnostics");
        let messages = parsed
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.clone())
            .collect::<Vec<_>>();
        assert!(messages.iter().any(|message| {
            message.contains("property block attribute `Constant` is not supported")
        }));
        assert!(messages.iter().any(|message| {
            message.contains("property block attribute `Dependent` is not supported")
        }));

        let unit = parsed.unit.expect("compilation unit");
        let Item::Class(class_def) = &unit.items[0] else {
            panic!("expected class item");
        };
        assert_eq!(class_def.property_blocks.len(), 1);
        assert_eq!(
            class_def.property_blocks[0].access,
            ClassMemberAccess::Private
        );
        assert_eq!(class_def.property_blocks[0].properties.len(), 1);
        assert_eq!(
            class_def.property_blocks[0].properties[0].name.name,
            "value"
        );
    }

    #[test]
    fn method_block_attributes_emit_explicit_diagnostics_for_unsupported_names() {
        let source = "classdef Point\n\
                      methods (Static, Sealed, Access=private, Abstract=true)\n\
                      function out = demo()\n\
                      out = 1;\n\
                      end\n\
                      end\n\
                      end\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::AutoDetect);
        assert!(parsed.has_errors(), "expected parse diagnostics");
        let messages = parsed
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.clone())
            .collect::<Vec<_>>();
        assert!(messages.iter().any(|message| {
            message.contains("method block attribute `Sealed` is not supported")
        }));
        assert!(messages.iter().any(|message| {
            message.contains("method block attribute `Abstract` is not supported")
        }));

        let unit = parsed.unit.expect("compilation unit");
        let Item::Class(class_def) = &unit.items[0] else {
            panic!("expected class item");
        };
        assert_eq!(class_def.method_blocks.len(), 1);
        assert!(class_def.method_blocks[0].is_static);
        assert_eq!(
            class_def.method_blocks[0].access,
            ClassMemberAccess::Private
        );
        assert_eq!(class_def.method_blocks[0].methods.len(), 1);
        assert_eq!(class_def.method_blocks[0].methods[0].name.name, "demo");
    }

    #[test]
    fn unsupported_events_blocks_do_not_break_later_methods_blocks() {
        let source = "classdef Point\n\
                      events (ListenAccess=private)\n\
                      Changed\n\
                      end\n\
                      methods\n\
                      function out = demo(obj)\n\
                      out = 1;\n\
                      end\n\
                      end\n\
                      end\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::AutoDetect);
        assert!(parsed.has_errors(), "expected parse diagnostics");
        let messages = parsed
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.clone())
            .collect::<Vec<_>>();
        assert!(messages
            .iter()
            .any(|message| message.contains("`events` blocks are not supported")));
        assert!(messages.iter().any(|message| {
            message.contains("`events` block attribute `ListenAccess` is not supported")
        }));

        let unit = parsed.unit.expect("compilation unit");
        let Item::Class(class_def) = &unit.items[0] else {
            panic!("expected class item");
        };
        assert_eq!(class_def.method_blocks.len(), 1);
        assert_eq!(class_def.method_blocks[0].methods.len(), 1);
        assert_eq!(class_def.method_blocks[0].methods[0].name.name, "demo");
    }

    #[test]
    fn unsupported_enumeration_blocks_do_not_break_later_methods_blocks() {
        let source = "classdef Point\n\
                      enumeration (Hidden=true)\n\
                      Red(1)\n\
                      end\n\
                      methods (Static)\n\
                      function out = demo()\n\
                      out = 1;\n\
                      end\n\
                      end\n\
                      end\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::AutoDetect);
        assert!(parsed.has_errors(), "expected parse diagnostics");
        let messages = parsed
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.clone())
            .collect::<Vec<_>>();
        assert!(messages
            .iter()
            .any(|message| message.contains("`enumeration` blocks are not supported")));
        assert!(messages.iter().any(|message| {
            message.contains("`enumeration` block attribute `Hidden` is not supported")
        }));

        let unit = parsed.unit.expect("compilation unit");
        let Item::Class(class_def) = &unit.items[0] else {
            panic!("expected class item");
        };
        assert_eq!(class_def.method_blocks.len(), 1);
        assert!(class_def.method_blocks[0].is_static);
        assert_eq!(class_def.method_blocks[0].methods.len(), 1);
        assert_eq!(class_def.method_blocks[0].methods[0].name.name, "demo");
    }

    #[test]
    fn unsupported_top_level_class_statements_do_not_break_later_methods_blocks() {
        let source = "classdef Point\n\
                      value = 1;\n\
                      methods\n\
                      function out = demo(obj)\n\
                      out = 1;\n\
                      end\n\
                      end\n\
                      end\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::AutoDetect);
        assert!(parsed.has_errors(), "expected parse diagnostics");
        assert!(parsed.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("top-level executable statements are not supported in class definitions")
        }));

        let unit = parsed.unit.expect("compilation unit");
        let Item::Class(class_def) = &unit.items[0] else {
            panic!("expected class item");
        };
        assert_eq!(class_def.method_blocks.len(), 1);
        assert_eq!(class_def.method_blocks[0].methods.len(), 1);
        assert_eq!(class_def.method_blocks[0].methods[0].name.name, "demo");
    }

    #[test]
    fn unsupported_property_validation_syntax_does_not_break_later_members() {
        let source = "classdef Point\n\
                      properties\n\
                      x (1,1) double\n\
                      y {mustBeNumeric}\n\
                      z = 3;\n\
                      end\n\
                      methods\n\
                      function out = demo(obj)\n\
                      out = obj.z;\n\
                      end\n\
                      end\n\
                      end\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::AutoDetect);
        assert!(parsed.has_errors(), "expected parse diagnostics");
        let validation_errors = parsed
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.message.contains(
                    "property validation, size/type constraints, and trailing declaration syntax are not supported",
                )
            })
            .count();
        assert_eq!(validation_errors, 2);

        let unit = parsed.unit.expect("compilation unit");
        let Item::Class(class_def) = &unit.items[0] else {
            panic!("expected class item");
        };
        assert_eq!(class_def.property_blocks.len(), 1);
        assert_eq!(class_def.property_blocks[0].properties.len(), 3);
        assert_eq!(class_def.property_blocks[0].properties[0].name.name, "x");
        assert_eq!(class_def.property_blocks[0].properties[1].name.name, "y");
        assert_eq!(class_def.property_blocks[0].properties[2].name.name, "z");
        assert_eq!(class_def.method_blocks.len(), 1);
        assert_eq!(class_def.method_blocks[0].methods.len(), 1);
        assert_eq!(class_def.method_blocks[0].methods[0].name.name, "demo");
    }

    #[test]
    fn unsupported_signature_only_method_declarations_do_not_break_later_methods() {
        let source = "classdef Point\n\
                      methods (Abstract)\n\
                      result = build(obj, x)\n\
                      end\n\
                      methods\n\
                      function out = demo(obj)\n\
                      out = 1;\n\
                      end\n\
                      end\n\
                      end\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::AutoDetect);
        assert!(parsed.has_errors(), "expected parse diagnostics");
        let messages = parsed
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.clone())
            .collect::<Vec<_>>();
        assert!(messages.iter().any(|message| {
            message.contains("method block attribute `Abstract` is not supported")
        }));
        assert!(messages.iter().any(|message| {
            message.contains(
                "methods blocks currently support only full `function ... end` method definitions",
            )
        }));

        let unit = parsed.unit.expect("compilation unit");
        let Item::Class(class_def) = &unit.items[0] else {
            panic!("expected class item");
        };
        assert_eq!(class_def.method_blocks.len(), 2);
        assert_eq!(class_def.method_blocks[0].methods.len(), 0);
        assert_eq!(class_def.method_blocks[1].methods.len(), 1);
        assert_eq!(class_def.method_blocks[1].methods[0].name.name, "demo");
    }

    #[test]
    fn static_false_attribute_does_not_mark_method_block_static() {
        let source = "classdef Point\n\
                      methods (Static=false, Access=private)\n\
                      function out = demo(obj)\n\
                      out = 1;\n\
                      end\n\
                      end\n\
                      end\n";
        let parsed = parse_source(source, SourceFileId(1), ParseMode::AutoDetect);
        assert!(parsed.has_errors(), "expected parse diagnostics");
        assert!(parsed.diagnostics.iter().any(|diagnostic| diagnostic
            .message
            .contains("`Static=true` is the only supported explicit Static form")));

        let unit = parsed.unit.expect("compilation unit");
        let Item::Class(class_def) = &unit.items[0] else {
            panic!("expected class item");
        };
        assert_eq!(class_def.method_blocks.len(), 1);
        assert!(!class_def.method_blocks[0].is_static);
        assert_eq!(
            class_def.method_blocks[0].access,
            ClassMemberAccess::Private
        );
        assert_eq!(class_def.method_blocks[0].methods.len(), 1);
        assert_eq!(class_def.method_blocks[0].methods[0].name.name, "demo");
    }
}
