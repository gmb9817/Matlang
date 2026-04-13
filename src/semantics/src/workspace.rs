//! Workspace and scope records for semantic binding.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkspaceId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    CompilationUnit,
    FunctionBody,
    AnonymousFunctionBody,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceKind {
    Script,
    Function,
    AnonymousFunction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub id: ScopeId,
    pub parent: Option<ScopeId>,
    pub kind: ScopeKind,
    pub workspace_id: WorkspaceId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub parent: Option<WorkspaceId>,
    pub kind: WorkspaceKind,
    pub scope_id: ScopeId,
    pub name: Option<String>,
}
