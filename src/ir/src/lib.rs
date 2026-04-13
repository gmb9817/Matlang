//! Intermediate representation crate for HIR, MIR, LIR, and verification.

pub mod hir;
pub mod lower;
pub mod testing;

pub use hir::{
    HirAnonymousFunction, HirAssignmentTarget, HirBinding, HirCallTarget, HirCallableRef,
    HirCapture, HirConditionalBranch, HirExpression, HirFunction, HirIndexArgument, HirItem,
    HirModule, HirStatement, HirSwitchCase, HirValueRef,
};
pub use lower::lower_to_hir;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrLevel {
    Hir,
    Mir,
    Lir,
}

pub fn summary() -> &'static str {
    "Owns HIR, MIR, LIR, pass interfaces, verifier logic, and IR serialization."
}
