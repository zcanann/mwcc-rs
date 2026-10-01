//! The hook that routes every function body through the staged PCode
//! pipeline (`docs/backend-pipeline-proposal.md`).
//!
//! The pipeline itself lives in `mwcc-pcode-path`, which the driver installs
//! with [`install_pcode_lowering`]; keeping it out of this crate's dependency
//! graph keeps the two building independently.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use mwcc_core::Compilation;
use mwcc_machine_code::MachineFunction;
use mwcc_syntax_trees::{Function, GlobalDeclaration, Type};
use mwcc_versions::CompilerConfig;

/// Everything the PCode pipeline needs about one function and its unit.
pub struct PcodeRequest<'a> {
    pub function: &'a Function,
    pub globals: &'a [GlobalDeclaration],
    /// Arrays declared at fixed addresses: name -> (address, element type).
    pub fixed_address_arrays: &'a HashMap<String, (i64, Type)>,
    /// Function statics, addressed like globals by name.
    pub static_locals: &'a [mwcc_syntax_trees::LocalDeclaration],
    pub call_return_types: &'a HashMap<String, Type>,
    /// The return type a pointer variable's callee has: (function, variable),
    /// with an empty function for a global.
    pub pointer_return_type: &'a dyn Fn(&str, &str) -> Option<Type>,
    pub variadic_callees: &'a HashSet<String>,
    pub prototyped: &'a HashSet<String>,
    pub call_parameter_types: &'a HashMap<String, Vec<Type>>,
    pub config: &'a CompilerConfig,
    /// Whether a call is to a compiler intrinsic (`name`, argument count).
    pub is_intrinsic: &'a dyn Fn(&str, usize) -> bool,
    /// Whether this unit has a body for the named function (a call MWCC
    /// may inline).
    pub has_body: &'a dyn Fn(&str) -> bool,
    /// Pointer parameters and locals whose pointee has no volatile storage.
    pub nonvolatile_pointers: &'a HashSet<String>,
    /// Pointer parameters and locals declared `const T *`.
    pub const_pointers: &'a HashSet<String>,
    /// The body MWCC expands for a call, when that expansion is modeled.
    pub inline_bodies: &'a HashMap<String, &'a Function>,
    /// The function returns C++ `bool` (stored like `unsigned char`).
    pub returns_bool: bool,
    /// The unit is C++ (comparisons produce `bool`).
    pub cxx: bool,
}

/// A PCode lowering entry point.
pub type PcodeLowering = fn(&PcodeRequest<'_>) -> Compilation<MachineFunction>;

static LOWERING: OnceLock<PcodeLowering> = OnceLock::new();

/// Install the PCode pipeline (once, before compiling).
pub fn install_pcode_lowering(lowering: PcodeLowering) {
    let _ = LOWERING.set(lowering);
}

pub(crate) fn lower(request: &PcodeRequest<'_>) -> Compilation<MachineFunction> {
    match LOWERING.get() {
        Some(lowering) => lowering(request),
        None => Err(mwcc_core::Diagnostic::error("the PCode lowering is not installed")),
    }
}

/// MWCC's inliner size: the callee's statements after lowering to jumps
/// (expressions, conditional and unconditional gotos, returns; not labels
/// or the final return). Measured against `-inline auto` limits.
pub(crate) fn inline_statement_count(function: &mwcc_syntax_trees::Function) -> usize {
    use mwcc_syntax_trees::{ArmBody, BinaryOperator, Expression, LoopKind, Statement, UnaryOperator};
    fn jumps(condition: &Expression) -> usize {
        match condition {
            Expression::Binary { operator: BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr, left, right } => {
                jumps(left) + jumps(right)
            }
            Expression::Unary { operator: UnaryOperator::LogicalNot, operand } => jumps(operand),
            _ => 1,
        }
    }
    fn statements(body: &[Statement]) -> usize {
        body.iter().map(statement).sum()
    }
    fn statement(statement: &Statement) -> usize {
        match statement {
            Statement::Store { .. } | Statement::Assign { .. } | Statement::Expression(_) | Statement::InlineAsm(_) => 1,
            Statement::Return(_) | Statement::Break | Statement::Continue | Statement::Goto(_) => 1,
            Statement::Label(_) => 0,
            Statement::If { condition, then_body, else_body } => {
                jumps(condition)
                    + statements(then_body)
                    + if else_body.is_empty() { 0 } else { 1 + statements(else_body) }
            }
            Statement::Switch { arms, default, .. } => {
                let arm = |body: &ArmBody| match body {
                    ArmBody::Return(_) => 1,
                    ArmBody::Statements(body) => statements(body),
                };
                1 + arms.iter().map(|a| arm(&a.body) + usize::from(!a.falls_through)).sum::<usize>()
                    + default.as_ref().map_or(0, arm)
            }
            Statement::Loop { kind, initializer, condition, step, body } => {
                let test = condition.as_ref().map_or(1, jumps);
                let entry = usize::from(*kind != LoopKind::DoWhile);
                usize::from(initializer.is_some()) + entry + statements(body) + usize::from(step.is_some()) + test
            }
        }
    }
    function.locals.iter().filter(|local| local.initializer.is_some()).count()
        + function.guards.iter().map(|guard| jumps(&guard.condition) + 1).sum::<usize>()
        + statements(&function.statements)
}

/// The largest callee `-inline auto` expands on this build (GC/3.x and Wii
/// shrink it for `,s`).
pub(crate) fn automatic_inline_limit(build_label: &str, size_goal: bool) -> usize {
    if build_label.starts_with("GC/1.3") || build_label.starts_with("GC/2.") {
        29
    } else if size_goal && (build_label.starts_with("GC/3.") || build_label.starts_with("Wii/")) {
        2
    } else {
        14
    }
}
