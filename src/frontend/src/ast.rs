//! Canonical AST skeleton for the Release 0.1 frontend.

use crate::source::SourceSpan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompilationUnitKind {
    Script,
    FunctionFile,
    ClassFile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilationUnit {
    pub kind: CompilationUnitKind,
    pub items: Vec<Item>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Statement(Statement),
    Function(FunctionDef),
    Class(ClassDef),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDef {
    pub name: Identifier,
    pub inputs: Vec<Identifier>,
    pub outputs: Vec<Identifier>,
    pub body: Vec<Statement>,
    pub local_functions: Vec<FunctionDef>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassDef {
    pub name: Identifier,
    pub superclass: Option<QualifiedName>,
    pub property_blocks: Vec<ClassPropertyBlock>,
    pub method_blocks: Vec<ClassMethodBlock>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassPropertyBlock {
    pub properties: Vec<ClassPropertyDef>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassPropertyDef {
    pub name: Identifier,
    pub default: Option<Expression>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassMethodBlock {
    pub methods: Vec<FunctionDef>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    pub kind: StatementKind,
    pub span: SourceSpan,
    pub display_suppressed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualifiedName {
    pub segments: Vec<Identifier>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatementKind {
    Assignment {
        targets: Vec<AssignmentTarget>,
        value: Expression,
        list_assignment: bool,
    },
    Expression(Expression),
    If {
        branches: Vec<ConditionalBranch>,
        else_body: Vec<Statement>,
    },
    Switch {
        expression: Expression,
        cases: Vec<SwitchCase>,
        otherwise_body: Vec<Statement>,
    },
    Try {
        body: Vec<Statement>,
        catch_binding: Option<Identifier>,
        catch_body: Vec<Statement>,
    },
    For {
        variable: Identifier,
        iterable: Expression,
        body: Vec<Statement>,
    },
    While {
        condition: Expression,
        body: Vec<Statement>,
    },
    Break,
    Continue,
    Return,
    Global(Vec<Identifier>),
    Persistent(Vec<Identifier>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionalBranch {
    pub condition: Expression,
    pub body: Vec<Statement>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchCase {
    pub matcher: Expression,
    pub body: Vec<Statement>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssignmentTarget {
    Identifier(Identifier),
    Index {
        target: Box<Expression>,
        indices: Vec<IndexArgument>,
    },
    CellIndex {
        target: Box<Expression>,
        indices: Vec<IndexArgument>,
    },
    Field {
        target: Box<Expression>,
        field: Identifier,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expression {
    pub kind: ExpressionKind,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpressionKind {
    Identifier(Identifier),
    NumberLiteral(String),
    CharLiteral(String),
    StringLiteral(String),
    MatrixLiteral(Vec<Vec<Expression>>),
    CellLiteral(Vec<Vec<Expression>>),
    FunctionHandle(QualifiedName),
    EndKeyword,
    Unary {
        op: UnaryOp,
        rhs: Box<Expression>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expression>,
        rhs: Box<Expression>,
    },
    Range {
        start: Box<Expression>,
        step: Option<Box<Expression>>,
        end: Box<Expression>,
    },
    ParenApply {
        target: Box<Expression>,
        indices: Vec<IndexArgument>,
    },
    CellIndex {
        target: Box<Expression>,
        indices: Vec<IndexArgument>,
    },
    FieldAccess {
        target: Box<Expression>,
        field: Identifier,
    },
    AnonymousFunction {
        params: Vec<Identifier>,
        body: Box<Expression>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexArgument {
    Expression(Expression),
    FullSlice,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Plus,
    Minus,
    LogicalNot,
    Transpose,
    DotTranspose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    MatrixRightDivide,
    MatrixLeftDivide,
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
    ShortCircuitAnd,
    ShortCircuitOr,
    Colon,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identifier {
    pub name: String,
    pub span: SourceSpan,
}
