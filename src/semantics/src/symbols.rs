//! Symbol records produced by semantic binding.

use std::path::PathBuf;

use matlab_frontend::ast::Expression;
use matlab_frontend::source::SourceSpan;

use crate::workspace::{ScopeId, WorkspaceId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymbolId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BindingId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Variable,
    Parameter,
    Output,
    Function,
    Class,
    Method,
    Property,
    Global,
    Persistent,
}

impl SymbolKind {
    pub fn is_function(self) -> bool {
        matches!(self, Self::Function | Self::Method)
    }

    pub fn is_capture_eligible(self) -> bool {
        matches!(
            self,
            Self::Variable | Self::Parameter | Self::Output | Self::Persistent
        )
    }

    pub fn binding_storage(self) -> Option<BindingStorage> {
        match self {
            Self::Variable | Self::Parameter | Self::Output => Some(BindingStorage::Local),
            Self::Global => Some(BindingStorage::Global),
            Self::Persistent => Some(BindingStorage::Persistent),
            Self::Function | Self::Class | Self::Method | Self::Property => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassPropertyInfo {
    pub name: String,
    pub default: Option<Expression>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalMethodInfo {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassInfo {
    pub name: String,
    pub package: Option<String>,
    pub inherits_handle: bool,
    pub properties: Vec<ClassPropertyInfo>,
    pub inline_methods: Vec<String>,
    pub external_methods: Vec<ExternalMethodInfo>,
    pub constructor: Option<String>,
    pub source_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub id: SymbolId,
    pub name: String,
    pub kind: SymbolKind,
    pub scope_id: ScopeId,
    pub workspace_id: WorkspaceId,
    pub binding_id: Option<BindingId>,
    pub declared_at: SourceSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingStorage {
    Local,
    Global,
    Persistent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub id: BindingId,
    pub name: String,
    pub storage: BindingStorage,
    pub owner_symbol: SymbolId,
    pub owner_scope: ScopeId,
    pub owner_workspace: WorkspaceId,
    pub shared_with_closures: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolReference {
    pub name: String,
    pub span: SourceSpan,
    pub role: ReferenceRole,
    pub resolution: ReferenceResolution,
    pub resolved_symbol: Option<SymbolId>,
    pub resolved_kind: Option<SymbolKind>,
    pub resolved_scope: Option<ScopeId>,
    pub resolved_workspace: Option<WorkspaceId>,
    pub binding_id: Option<BindingId>,
    pub capture_access: Option<CaptureAccess>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capture {
    pub name: String,
    pub binding_id: BindingId,
    pub access: CaptureAccess,
    pub from_symbol: SymbolId,
    pub from_scope: ScopeId,
    pub from_workspace: WorkspaceId,
    pub into_scope: ScopeId,
    pub into_workspace: WorkspaceId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureAccess {
    Read,
    Write,
    ReadWrite,
}

impl CaptureAccess {
    pub fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::ReadWrite, _) | (_, Self::ReadWrite) => Self::ReadWrite,
            (Self::Read, Self::Write) | (Self::Write, Self::Read) => Self::ReadWrite,
            (Self::Read, Self::Read) => Self::Read,
            (Self::Write, Self::Write) => Self::Write,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceRole {
    Value,
    CallTarget,
    FunctionHandleTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferenceResolution {
    WorkspaceValue,
    BuiltinValue,
    NestedFunction,
    FileFunction,
    BuiltinFunction,
    ExternalFunctionCandidate,
    UnresolvedValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedReference {
    pub name: String,
    pub span: SourceSpan,
    pub role: ReferenceRole,
    pub semantic_resolution: ReferenceResolution,
    pub final_resolution: FinalReferenceResolution,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinalReferenceResolution {
    WorkspaceValue,
    BuiltinValue,
    NestedFunction,
    FileFunction,
    BuiltinFunction,
    ResolvedPath {
        kind: PathResolutionKind,
        path: PathBuf,
        package: Option<String>,
        shadowed_builtin: bool,
    },
    UnresolvedExternal,
    UnresolvedValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathResolutionKind {
    PrivateDirectory,
    CurrentDirectory,
    SearchPath,
    PackageDirectory,
    ClassCurrentDirectory,
    ClassSearchPath,
    ClassPackageDirectory,
    ClassFolderCurrentDirectory,
    ClassFolderSearchPath,
    ClassFolderPackageDirectory,
}
