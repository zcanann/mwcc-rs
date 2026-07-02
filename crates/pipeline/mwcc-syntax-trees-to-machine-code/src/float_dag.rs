//! The FLOAT DAG return arm: a `double` function returning a pure
//! multiply-add tree of double parameters and pooled double constants runs
//! through the FROZEN float models (fires 331-336): the linearizer's float
//! contract (HAZARD_FPU single pipe, the load port, the blocked-load stall
//! and empty-cycle lift) orders the body, and the hybrid float register
//! machine (reverse death-order allocation with boundary shares) assigns the
//! FPRs. Captured vocabulary ONLY: fmadd (contracted under fp_contract) and
//! fmul — a tree with an unfused add, subtract, divide, negate, memory load,
//! duplicate literal, or constant-folded pair DEFERS.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, Type};
use mwcc_vreg::{assign_float_registers, linearize, DagNode, FROZEN_FLOAT_REG, HAZARD_FPU};
use crate::generator::*;

/// A value in the lowered tree: a parameter's DAG value id or a node index.
#[derive(Clone, Copy)]
enum Operand {
    Param(u32),
    Node(usize),
}

/// The recursive tree after fmadd contraction, before ordering.
enum Tree {
    Param(u32),
    Const(u64),
    /// factor_left * factor_right + addend (fp_contract).
    Madd { factor_left: Box<Tree>, factor_right: Box<Tree>, addend: Box<Tree> },
    Mul { left: Box<Tree>, right: Box<Tree> },
}

/// One emitted node, operands in the final instruction slots (the measured
/// convention: a CONSTANT factor takes the A slot, otherwise source order).
enum FloatOp {
    Const(u64),
    Madd { a: Operand, c: Operand, b: Operand },
    Mul { a: Operand, c: Operand },
}

const LOAD_LATENCY: u32 = 2;
const FLOAT_ARITH_LATENCY: u32 = 3;

impl Generator {
    /// Claim `return <double multiply-add tree>;` for the frozen float
    /// models. Returns whether it emitted the body (the caller appends the
    /// epilogue/blr).
    pub(crate) fn try_float_dag_return(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type != Type::Double
            || !function.statements.is_empty()
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let Some(return_expression) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        // Every parameter must be a double already living in its FPR.
        let mut params: Vec<(u32, u8)> = Vec::new();
        let mut param_ids: Vec<(String, u32)> = Vec::new();
        for (index, parameter) in function.parameters.iter().enumerate() {
            if parameter.parameter_type != Type::Double {
                return Ok(false);
            }
            let Some(location) = self.locations.get(&parameter.name) else {
                return Ok(false);
            };
            if location.class != ValueClass::Float || location.width != 64 {
                return Ok(false);
            }
            let value = (index + 1) as u32;
            params.push((value, location.register));
            param_ids.push((parameter.name.clone(), value));
        }
        // Lower the expression to the contracted tree (or bail).
        let mut seen_literals: Vec<u64> = Vec::new();
        let Some(tree) = build_tree(return_expression, &param_ids, &mut seen_literals) else {
            return Ok(false);
        };
        // Order the arith nodes by (tree level DESC, factor-side first) and
        // group each node's constant loads before the arith block — the
        // measured construction the frozen linearizer was fitted against.
        let mut arith_refs: Vec<(&Tree, u32)> = Vec::new();
        collect_arith(&tree, 0, &mut arith_refs);
        if arith_refs.len() < 2 {
            // Single-op float returns stay on the existing verified path.
            return Ok(false);
        }
        arith_refs.sort_by_key(|&(_, level)| std::cmp::Reverse(level));

        let mut nodes: Vec<DagNode> = Vec::new();
        let mut ops: Vec<FloatOp> = Vec::new();
        // Map from each arith Tree's address to its node operand.
        let mut built: Vec<(*const Tree, Operand)> = Vec::new();
        // Pass 1: pooled constant loads, grouped per consumer (factor first).
        for &(arith, _) in &arith_refs {
            let mut push_const = |bits: u64, nodes: &mut Vec<DagNode>, ops: &mut Vec<FloatOp>, built: &mut Vec<(*const Tree, Operand)>, key: *const Tree| {
                let index = nodes.len();
                nodes.push(DagNode::new("lfd", LOAD_LATENCY).writes(&[10 + index as u32]));
                ops.push(FloatOp::Const(bits));
                built.push((key, Operand::Node(index)));
            };
            match arith {
                Tree::Madd { factor_left, factor_right, addend } => {
                    for factor in [factor_left, factor_right] {
                        if let Tree::Const(bits) = factor.as_ref() {
                            push_const(*bits, &mut nodes, &mut ops, &mut built, factor.as_ref() as *const Tree);
                        }
                    }
                    if let Tree::Const(bits) = addend.as_ref() {
                        push_const(*bits, &mut nodes, &mut ops, &mut built, addend.as_ref() as *const Tree);
                    }
                }
                Tree::Mul { .. } => {
                    // A constant Mul factor is uncaptured (only fmadd has the
                    // const-in-A evidence) — build_tree already deferred it.
                }
                _ => unreachable!("collect_arith yields arith nodes only"),
            }
        }
        // Pass 2: the arith nodes themselves (deepest level first).
        for &(arith, _) in &arith_refs {
            let resolve = |subtree: &Tree, built: &[(*const Tree, Operand)]| -> Option<Operand> {
                match subtree {
                    Tree::Param(value) => Some(Operand::Param(*value)),
                    _ => built
                        .iter()
                        .rev()
                        .find(|(key, _)| std::ptr::eq(*key, subtree as *const Tree))
                        .map(|&(_, operand)| operand),
                }
            };
            let value_of = |operand: Operand, nodes: &[DagNode]| -> u32 {
                match operand {
                    Operand::Param(value) => value,
                    Operand::Node(index) => nodes[index].writes[0],
                }
            };
            let index = nodes.len();
            match arith {
                Tree::Madd { factor_left, factor_right, addend } => {
                    let left = resolve(factor_left, &built).ok_or_else(|| Diagnostic::error("float DAG operand resolution"))?;
                    let right = resolve(factor_right, &built).ok_or_else(|| Diagnostic::error("float DAG operand resolution"))?;
                    let b = resolve(addend, &built).ok_or_else(|| Diagnostic::error("float DAG operand resolution"))?;
                    // The measured slot convention: a pooled-constant factor
                    // takes A; otherwise source order stands.
                    let (a, c) = if matches!(factor_right.as_ref(), Tree::Const(_)) { (right, left) } else { (left, right) };
                    let reads: Vec<u32> = [a, c, b].iter().map(|&operand| value_of(operand, &nodes)).collect();
                    nodes.push(
                        DagNode::new("fmadd", FLOAT_ARITH_LATENCY)
                            .hazard(HAZARD_FPU)
                            .reads(&reads)
                            .writes(&[10 + index as u32]),
                    );
                    ops.push(FloatOp::Madd { a, c, b });
                }
                Tree::Mul { left, right } => {
                    let a = resolve(left, &built).ok_or_else(|| Diagnostic::error("float DAG operand resolution"))?;
                    let c = resolve(right, &built).ok_or_else(|| Diagnostic::error("float DAG operand resolution"))?;
                    let reads: Vec<u32> = [a, c].iter().map(|&operand| value_of(operand, &nodes)).collect();
                    nodes.push(
                        DagNode::new("fmul", FLOAT_ARITH_LATENCY)
                            .hazard(HAZARD_FPU)
                            .reads(&reads)
                            .writes(&[10 + index as u32]),
                    );
                    ops.push(FloatOp::Mul { a, c });
                }
                _ => unreachable!(),
            }
            built.push((arith as *const Tree, Operand::Node(index)));
        }

        let order = linearize(&nodes);
        let registers = assign_float_registers(&nodes, &order, &params, FROZEN_FLOAT_REG);
        if registers.iter().any(|register| register.is_none()) {
            return Ok(false);
        }
        // mwcc pools the constants in SOURCE-appearance order (measured:
        // horner3's .sdata2 is 1.5, 2.5, 3.5 while the lfd order is the
        // reverse) — intern them in a left-to-right expression walk before
        // emission references them (build_tree visits factors first, so
        // seen_literals is NOT source order).
        let mut source_literals: Vec<u64> = Vec::new();
        collect_literals(return_expression, &mut source_literals);
        for &bits in &source_literals {
            self.output.intern_constant(bits, 8);
        }
        let register_of = |operand: Operand| -> u8 {
            match operand {
                Operand::Param(value) => params.iter().find(|&&(v, _)| v == value).map(|&(_, register)| register).unwrap_or(1),
                Operand::Node(index) => registers[index].unwrap_or(1),
            }
        };
        for &node in &order {
            let d = registers[node].expect("checked above");
            match &ops[node] {
                FloatOp::Const(bits) => self.load_double_constant(d, *bits),
                FloatOp::Madd { a, c, b } => self.output.instructions.push(Instruction::FloatMultiplyAddDouble {
                    d,
                    a: register_of(*a),
                    c: register_of(*c),
                    b: register_of(*b),
                }),
                FloatOp::Mul { a, c } => self.output.instructions.push(Instruction::FloatMultiplyDouble {
                    d,
                    a: register_of(*a),
                    c: register_of(*c),
                }),
            }
        }
        Ok(true)
    }
}

/// Lower an expression to the contracted tree. `None` defers: anything
/// outside the captured vocabulary (params + distinct double literals
/// combined by fmadd/fmul).
fn build_tree(expression: &Expression, params: &[(String, u32)], seen_literals: &mut Vec<u64>) -> Option<Tree> {
    match expression {
        Expression::Variable(name) => {
            let &(_, value) = params.iter().find(|(parameter, _)| parameter == name)?;
            Some(Tree::Param(value))
        }
        Expression::FloatLiteral(value) => {
            let bits = value.to_bits();
            // A duplicated literal's pool/reuse behavior is uncaptured.
            if seen_literals.contains(&bits) {
                return None;
            }
            seen_literals.push(bits);
            Some(Tree::Const(bits))
        }
        Expression::Binary { operator: BinaryOperator::Add, left, right } => {
            // fp_contract: fuse a multiplication side into fmadd. When BOTH
            // sides multiply, the LEFT fuses and the right evaluates as an
            // fmul addend (measured: s1_s2 and s1_s2_shallow both emit
            // fmadd f1,f1,<s1 chain>,<fmul of s2>).
            let left_mul = matches!(left.as_ref(), Expression::Binary { operator: BinaryOperator::Multiply, .. });
            let right_mul = matches!(right.as_ref(), Expression::Binary { operator: BinaryOperator::Multiply, .. });
            match (left_mul, right_mul) {
                (false, false) => None,
                (true, _) => {
                    let Expression::Binary { left: x, right: y, .. } = left.as_ref() else { unreachable!() };
                    make_madd(x, y, right, params, seen_literals)
                }
                (false, true) => {
                    let Expression::Binary { left: x, right: y, .. } = right.as_ref() else { unreachable!() };
                    make_madd(x, y, left, params, seen_literals)
                }
            }
        }
        Expression::Binary { operator: BinaryOperator::Multiply, left, right } => {
            // A constant fmul factor is uncaptured (const-in-A evidence
            // exists only for fmadd); constant folding likewise. A factor
            // that is ITSELF a multiplication is uncaptured too (probed:
            // (z*w)*(1.5+z*2.5) schedules the inner fmul into the load
            // window and swaps the register chains — a DIFF, so defer).
            let is_mul = |side: &Expression| matches!(side, Expression::Binary { operator: BinaryOperator::Multiply, .. });
            if matches!(left.as_ref(), Expression::FloatLiteral(_))
                || matches!(right.as_ref(), Expression::FloatLiteral(_))
                || is_mul(left)
                || is_mul(right)
            {
                return None;
            }
            let left = build_tree(left, params, seen_literals)?;
            let right = build_tree(right, params, seen_literals)?;
            Some(Tree::Mul { left: Box::new(left), right: Box::new(right) })
        }
        _ => None,
    }
}

/// Build an fmadd from `x*y + addend`, deferring constant-foldable pairs.
fn make_madd(x: &Expression, y: &Expression, addend: &Expression, params: &[(String, u32)], seen_literals: &mut Vec<u64>) -> Option<Tree> {
    let both_const = matches!(x, Expression::FloatLiteral(_)) && matches!(y, Expression::FloatLiteral(_));
    if both_const {
        return None;
    }
    let factor_left = build_tree(x, params, seen_literals)?;
    let factor_right = build_tree(y, params, seen_literals)?;
    let addend = build_tree(addend, params, seen_literals)?;
    Some(Tree::Madd { factor_left: Box::new(factor_left), factor_right: Box::new(factor_right), addend: Box::new(addend) })
}

/// Collect double literals in source (left-to-right) order — the measured
/// .sdata2 pool order.
fn collect_literals(expression: &Expression, into: &mut Vec<u64>) {
    match expression {
        Expression::FloatLiteral(value) => into.push(value.to_bits()),
        Expression::Binary { left, right, .. } => {
            collect_literals(left, into);
            collect_literals(right, into);
        }
        _ => {}
    }
}

/// Collect arith nodes with their tree level (root 0), factor subtrees
/// before the addend so the stable level sort keeps factor-side-first ties —
/// the measured evaluation order.
fn collect_arith<'tree>(tree: &'tree Tree, level: u32, into: &mut Vec<(&'tree Tree, u32)>) {
    match tree {
        Tree::Madd { factor_left, factor_right, addend } => {
            into.push((tree, level));
            collect_arith(factor_left, level + 1, into);
            collect_arith(factor_right, level + 1, into);
            collect_arith(addend, level + 1, into);
        }
        Tree::Mul { left, right } => {
            into.push((tree, level));
            collect_arith(left, level + 1, into);
            collect_arith(right, level + 1, into);
        }
        Tree::Param(_) | Tree::Const(_) => {}
    }
}
