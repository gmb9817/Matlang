use matlab_frontend::ast::{BinaryOp, CompilationUnitKind, UnaryOp};
use matlab_semantics::{
    symbols::{
        BindingId, BindingStorage, CaptureAccess, FinalReferenceResolution, ReferenceResolution,
        SymbolId, SymbolKind,
    },
    workspace::{ScopeId, WorkspaceId},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirModule {
    pub kind: CompilationUnitKind,
    pub scope_id: ScopeId,
    pub workspace_id: WorkspaceId,
    pub implicit_ans: Option<HirBinding>,
    pub items: Vec<HirItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HirItem {
    Statement(HirStatement),
    Function(HirFunction),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirFunction {
    pub name: String,
    pub scope_id: ScopeId,
    pub workspace_id: WorkspaceId,
    pub implicit_ans: Option<HirBinding>,
    pub inputs: Vec<HirBinding>,
    pub outputs: Vec<HirBinding>,
    pub captures: Vec<HirCapture>,
    pub body: Vec<HirStatement>,
    pub local_functions: Vec<HirFunction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirAnonymousFunction {
    pub scope_id: ScopeId,
    pub workspace_id: WorkspaceId,
    pub params: Vec<HirBinding>,
    pub captures: Vec<HirCapture>,
    pub body: Box<HirExpression>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirBinding {
    pub name: String,
    pub symbol_kind: SymbolKind,
    pub binding_id: Option<BindingId>,
    pub storage: Option<BindingStorage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirCapture {
    pub name: String,
    pub binding_id: BindingId,
    pub access: CaptureAccess,
    pub from_scope: ScopeId,
    pub from_workspace: WorkspaceId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HirStatement {
    Assignment {
        targets: Vec<HirAssignmentTarget>,
        value: HirExpression,
        display_suppressed: bool,
    },
    Expression {
        expression: HirExpression,
        display_suppressed: bool,
    },
    If {
        branches: Vec<HirConditionalBranch>,
        else_body: Vec<HirStatement>,
    },
    Switch {
        expression: HirExpression,
        cases: Vec<HirSwitchCase>,
        otherwise_body: Vec<HirStatement>,
    },
    Try {
        body: Vec<HirStatement>,
        catch_binding: Option<HirBinding>,
        catch_body: Vec<HirStatement>,
    },
    For {
        variable: HirBinding,
        iterable: HirExpression,
        body: Vec<HirStatement>,
    },
    While {
        condition: HirExpression,
        body: Vec<HirStatement>,
    },
    Break,
    Continue,
    Return,
    Global(Vec<HirBinding>),
    Persistent(Vec<HirBinding>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirConditionalBranch {
    pub condition: HirExpression,
    pub body: Vec<HirStatement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirSwitchCase {
    pub matcher: HirExpression,
    pub body: Vec<HirStatement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HirAssignmentTarget {
    Binding(HirBinding),
    Index {
        target: Box<HirExpression>,
        indices: Vec<HirIndexArgument>,
    },
    CellIndex {
        target: Box<HirExpression>,
        indices: Vec<HirIndexArgument>,
    },
    Field {
        target: Box<HirExpression>,
        field: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HirExpression {
    ValueRef(HirValueRef),
    NumberLiteral(String),
    CharLiteral(String),
    StringLiteral(String),
    MatrixLiteral(Vec<Vec<HirExpression>>),
    CellLiteral(Vec<Vec<HirExpression>>),
    FunctionHandle(HirCallableRef),
    EndKeyword,
    Unary {
        op: UnaryOp,
        rhs: Box<HirExpression>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<HirExpression>,
        rhs: Box<HirExpression>,
    },
    Range {
        start: Box<HirExpression>,
        step: Option<Box<HirExpression>>,
        end: Box<HirExpression>,
    },
    Call {
        target: HirCallTarget,
        args: Vec<HirIndexArgument>,
    },
    CellIndex {
        target: Box<HirExpression>,
        indices: Vec<HirIndexArgument>,
    },
    FieldAccess {
        target: Box<HirExpression>,
        field: String,
    },
    AnonymousFunction(HirAnonymousFunction),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HirCallTarget {
    Callable(HirCallableRef),
    Expression(Box<HirExpression>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HirIndexArgument {
    Expression(HirExpression),
    FullSlice,
    End,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirValueRef {
    pub name: String,
    pub resolution: ReferenceResolution,
    pub binding_id: Option<BindingId>,
    pub symbol_kind: Option<SymbolKind>,
    pub capture_access: Option<CaptureAccess>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirCallableRef {
    pub name: String,
    pub semantic_resolution: ReferenceResolution,
    pub final_resolution: Option<FinalReferenceResolution>,
    pub resolved_symbol: Option<SymbolId>,
    pub resolved_kind: Option<SymbolKind>,
    pub binding_id: Option<BindingId>,
    pub capture_access: Option<CaptureAccess>,
}
