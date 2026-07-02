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
    /// A named double local's shared node (by locals-list index).
    LocalRef(usize),
    Const(u64),
    /// factor_left * factor_right + addend (fp_contract).
    Madd { factor_left: Box<Tree>, factor_right: Box<Tree>, addend: Box<Tree> },
    /// base - factor_left * factor_right (fp_contract: fnmsub d,a,c,b = b - a*c).
    Fnmsub { factor_left: Box<Tree>, factor_right: Box<Tree>, base: Box<Tree> },
    /// factor_left * factor_right - subtrahend (fp_contract: fmsub d,a,c,b = a*c - b).
    Fmsub { factor_left: Box<Tree>, factor_right: Box<Tree>, subtrahend: Box<Tree> },
    Mul { left: Box<Tree>, right: Box<Tree> },
}

/// One emitted node, operands in the final instruction slots (the measured
/// convention: a CONSTANT factor takes the A slot, otherwise source order).
enum FloatOp {
    Const(u64),
    Madd { a: Operand, c: Operand, b: Operand },
    Fnmsub { a: Operand, c: Operand, b: Operand },
    Fmsub { a: Operand, c: Operand, b: Operand },
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
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        // Named double locals are WINDOW-TOP tier values (measured: k_sin's
        // z/v take f4/f3): each must be a plain scalar double with a
        // single-arith initializer over params and prior locals.
        for local in &function.locals {
            if local.declared_type != Type::Double
                || local.initializer.is_none()
                || local.array_length.is_some()
            {
                return Ok(false);
            }
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
        let local_names: Vec<(String, usize)> = function
            .locals
            .iter()
            .enumerate()
            .map(|(index, local)| (local.name.clone(), index))
            .collect();
        // Each local's initializer must lower to exactly ONE arith node over
        // params and PRIOR locals (constants in inits are uncaptured).
        let mut local_trees: Vec<Tree> = Vec::new();
        for (index, local) in function.locals.iter().enumerate() {
            let initializer = local.initializer.as_ref().expect("gated above");
            let prior = &local_names[..index];
            let Some(local_tree) = build_tree(initializer, &param_ids, prior, &mut seen_literals) else {
                return Ok(false);
            };
            if count_arith(&local_tree) != 1 || !seen_literals.is_empty() {
                return Ok(false);
            }
            local_trees.push(local_tree);
        }
        let Some(tree) = build_tree(return_expression, &param_ids, &local_names, &mut seen_literals) else {
            return Ok(false);
        };
        // Local shapes beyond the captured range defer: a >= 4-arith return
        // tree over locals diverges (probed: the 4-coefficient z-chain), and
        // a local consumed ONLY by the return root alongside a >= 2-arith
        // chain is held back by mwcc's scheduler (the v=z*x fmul waits past
        // the chain's first fmadd — a far-consumer stall the order model
        // does not fit yet).
        if !function.locals.is_empty() {
            let return_arith_total = {
                let mut refs: Vec<(&Tree, u32)> = Vec::new();
                collect_arith(&tree, 0, &mut refs);
                refs.len()
            };
            if return_arith_total >= 4 {
                return Ok(false);
            }
            let uses = |index: usize, tree: &Tree| -> usize {
                fn walk(tree: &Tree, index: usize, count: &mut usize) {
                    match tree {
                        Tree::LocalRef(local) if *local == index => *count += 1,
                        Tree::Madd { factor_left, factor_right, addend }
                        | Tree::Fnmsub { factor_left, factor_right, base: addend }
                        | Tree::Fmsub { factor_left, factor_right, subtrahend: addend } => {
                            walk(factor_left, index, count);
                            walk(factor_right, index, count);
                            walk(addend, index, count);
                        }
                        Tree::Mul { left, right } => {
                            walk(left, index, count);
                            walk(right, index, count);
                        }
                        _ => {}
                    }
                }
                let mut count = 0;
                walk(tree, index, &mut count);
                count
            };
            let return_arith = {
                let mut refs: Vec<(&Tree, u32)> = Vec::new();
                collect_arith(&tree, 0, &mut refs);
                refs.len()
            };
            for (index, _) in function.locals.iter().enumerate() {
                let root_only = match &tree {
                    Tree::Madd { factor_left, factor_right, addend } => {
                        let direct = [factor_left, factor_right, addend]
                            .iter()
                            .filter(|sub| matches!(sub.as_ref(), Tree::LocalRef(local) if *local == index))
                            .count();
                        direct == uses(index, &tree)
                    }
                    Tree::Mul { left, right } => {
                        let direct = [left, right]
                            .iter()
                            .filter(|sub| matches!(sub.as_ref(), Tree::LocalRef(local) if *local == index))
                            .count();
                        direct == uses(index, &tree)
                    }
                    _ => false,
                };
                let later_local_uses: usize = local_trees
                    .iter()
                    .skip(index + 1)
                    .map(|later| uses(index, later))
                    .sum();
                if root_only && later_local_uses == 0 && uses(index, &tree) > 0 && return_arith >= 3 {
                    return Ok(false);
                }
            }
        }
        // Order the arith nodes by (tree level DESC, factor-side first) and
        // group each node's constant loads before the arith block — the
        // measured construction the frozen linearizer was fitted against.
        let mut arith_refs: Vec<(&Tree, u32)> = Vec::new();
        collect_arith(&tree, 0, &mut arith_refs);
        if arith_refs.is_empty() || (arith_refs.len() < 2 && seen_literals.is_empty()) {
            // A bare constant return and const-free single ops stay on the
            // existing verified paths; a pooled-constant single op is ours.
            return Ok(false);
        }
        arith_refs.sort_by_key(|&(_, level)| std::cmp::Reverse(level));

        let mut nodes: Vec<DagNode> = Vec::new();
        let mut ops: Vec<FloatOp> = Vec::new();
        // Map from each arith Tree's address to its node operand.
        let mut built: Vec<(*const Tree, Operand)> = Vec::new();
        // The locals' shared nodes, in declaration order (their arith emits
        // ahead of the loads: measured, z's fmul is slot 0).
        let mut local_operands: Vec<Operand> = Vec::new();
        for local_tree in &local_trees {
            let resolve_leaf = |leaf: &Tree| -> Option<Operand> {
                match leaf {
                    Tree::Param(value) => Some(Operand::Param(*value)),
                    Tree::LocalRef(local) => local_operands.get(*local).copied(),
                    _ => None,
                }
            };
            let index = nodes.len();
            let (op, reads) = match local_tree {
                Tree::Mul { left, right } => {
                    let a = resolve_leaf(left).ok_or_else(|| Diagnostic::error("float local operand"))?;
                    let c = resolve_leaf(right).ok_or_else(|| Diagnostic::error("float local operand"))?;
                    (FloatOp::Mul { a, c }, vec![a, c])
                }
                Tree::Madd { factor_left, factor_right, addend } => {
                    let a = resolve_leaf(factor_left).ok_or_else(|| Diagnostic::error("float local operand"))?;
                    let c = resolve_leaf(factor_right).ok_or_else(|| Diagnostic::error("float local operand"))?;
                    let b = resolve_leaf(addend).ok_or_else(|| Diagnostic::error("float local operand"))?;
                    (FloatOp::Madd { a, c, b }, vec![a, c, b])
                }
                _ => return Ok(false),
            };
            let read_values: Vec<u32> = reads
                .iter()
                .map(|&operand| match operand {
                    Operand::Param(value) => value,
                    Operand::Node(node) => nodes[node].writes[0],
                })
                .collect();
            nodes.push(
                DagNode::new("flocal", FLOAT_ARITH_LATENCY)
                    .hazard(HAZARD_FPU)
                    .local_home()
                    .reads(&read_values)
                    .writes(&[10 + index as u32]),
            );
            ops.push(op);
            let operand = Operand::Node(index);
            local_operands.push(operand);
            built.push((local_tree as *const Tree, operand));
        }
        // Pass 1: pooled constant loads, grouped per consumer (factor first).
        for &(arith, _) in &arith_refs {
            let mut push_const = |bits: u64, nodes: &mut Vec<DagNode>, ops: &mut Vec<FloatOp>, built: &mut Vec<(*const Tree, Operand)>, key: *const Tree| {
                let index = nodes.len();
                nodes.push(DagNode::new("lfd", LOAD_LATENCY).writes(&[10 + index as u32]));
                ops.push(FloatOp::Const(bits));
                built.push((key, Operand::Node(index)));
            };
            match arith {
                Tree::Madd { factor_left, factor_right, addend }
                | Tree::Fnmsub { factor_left, factor_right, base: addend }
                | Tree::Fmsub { factor_left, factor_right, subtrahend: addend } => {
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
                    Tree::LocalRef(local) => local_operands.get(*local).copied(),
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
                Tree::Fmsub { factor_left, factor_right, subtrahend } => {
                    let a = resolve(factor_left, &built).ok_or_else(|| Diagnostic::error("float DAG operand resolution"))?;
                    let c = resolve(factor_right, &built).ok_or_else(|| Diagnostic::error("float DAG operand resolution"))?;
                    let b = resolve(subtrahend, &built).ok_or_else(|| Diagnostic::error("float DAG operand resolution"))?;
                    // Measured: the source-left factor keeps A (constant
                    // factors are deferred at build).
                    let reads: Vec<u32> = [a, c, b].iter().map(|&operand| value_of(operand, &nodes)).collect();
                    nodes.push(
                        DagNode::new("fmsub", FLOAT_ARITH_LATENCY)
                            .hazard(HAZARD_FPU)
                            .reads(&reads)
                            .writes(&[10 + index as u32]),
                    );
                    ops.push(FloatOp::Fmsub { a, c, b });
                }
                Tree::Fnmsub { factor_left, factor_right, base } => {
                    let left = resolve(factor_left, &built).ok_or_else(|| Diagnostic::error("float DAG operand resolution"))?;
                    let right = resolve(factor_right, &built).ok_or_else(|| Diagnostic::error("float DAG operand resolution"))?;
                    let b = resolve(base, &built).ok_or_else(|| Diagnostic::error("float DAG operand resolution"))?;
                    // The same measured slot convention as fmadd: a pooled
                    // constant factor takes A (fnmsub f1,f2,f1,f0).
                    let (a, c) = if matches!(factor_right.as_ref(), Tree::Const(_)) { (right, left) } else { (left, right) };
                    let reads: Vec<u32> = [a, c, b].iter().map(|&operand| value_of(operand, &nodes)).collect();
                    nodes.push(
                        DagNode::new("fnmsub", FLOAT_ARITH_LATENCY)
                            .hazard(HAZARD_FPU)
                            .reads(&reads)
                            .writes(&[10 + index as u32]),
                    );
                    ops.push(FloatOp::Fnmsub { a, c, b });
                }
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
                FloatOp::Fnmsub { a, c, b } => self.output.instructions.push(Instruction::FloatNegativeMultiplySubtractDouble {
                    d,
                    a: register_of(*a),
                    c: register_of(*c),
                    b: register_of(*b),
                }),
                FloatOp::Fmsub { a, c, b } => self.output.instructions.push(Instruction::FloatMultiplySubtractDouble {
                    d,
                    a: register_of(*a),
                    c: register_of(*c),
                    b: register_of(*b),
                }),
                FloatOp::Mul { a, c } => {
                    // Measured fmul slots: a PARAM-only product keeps source
                    // order (fmul f0,f1,f2 for z*w); any VALUE operand sorts
                    // the registers DESCENDING into A (every captured root
                    // and chain fmul complies — shallow A=f1>f0, deep A=f4>f0).
                    let (mut ra, mut rc) = (register_of(*a), register_of(*c));
                    let both_params = matches!(a, Operand::Param(_)) && matches!(c, Operand::Param(_));
                    if !both_params && rc > ra {
                        std::mem::swap(&mut ra, &mut rc);
                    }
                    self.output.instructions.push(Instruction::FloatMultiplyDouble { d, a: ra, c: rc });
                }
            }
        }
        Ok(true)
    }
}

/// Lower an expression to the contracted tree. `None` defers: anything
/// outside the captured vocabulary (params + distinct double literals
/// combined by fmadd/fmul).
fn build_tree(
    expression: &Expression,
    params: &[(String, u32)],
    locals: &[(String, usize)],
    seen_literals: &mut Vec<u64>,
) -> Option<Tree> {
    match expression {
        Expression::Variable(name) => {
            if let Some(&(_, index)) = locals.iter().find(|(local, _)| local == name) {
                return Some(Tree::LocalRef(index));
            }
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
                    make_madd(x, y, right, params, locals, seen_literals)
                }
                (false, true) => {
                    let Expression::Binary { left: x, right: y, .. } = right.as_ref() else { unreachable!() };
                    make_madd(x, y, left, params, locals, seen_literals)
                }
            }
        }
        Expression::Binary { operator: BinaryOperator::Subtract, left, right } => {
            // fp_contract: `b - x*y` contracts to fnmsub, `x*y - b` to
            // fmsub (measured: the root-slot order + dying-door rules fit
            // the simple, deep, and wmul fmsub roots). A constant fmsub
            // FACTOR is uncaptured — deferred inside the branch.
            if let Expression::Binary { operator: BinaryOperator::Multiply, left: x, right: y } = left.as_ref() {
                if matches!(x.as_ref(), Expression::FloatLiteral(_)) || matches!(y.as_ref(), Expression::FloatLiteral(_)) {
                    return None;
                }
                let factor_left = build_tree(x, params, locals, seen_literals)?;
                let factor_right = build_tree(y, params, locals, seen_literals)?;
                let subtrahend = build_tree(right, params, locals, seen_literals)?;
                return Some(Tree::Fmsub {
                    factor_left: Box::new(factor_left),
                    factor_right: Box::new(factor_right),
                    subtrahend: Box::new(subtrahend),
                });
            }
            let Expression::Binary { operator: BinaryOperator::Multiply, left: x, right: y } = right.as_ref() else {
                return None;
            };
            let both_const = matches!(x.as_ref(), Expression::FloatLiteral(_)) && matches!(y.as_ref(), Expression::FloatLiteral(_));
            if both_const {
                return None;
            }
            let base = build_tree(left, params, locals, seen_literals)?;
            let factor_left = build_tree(x, params, locals, seen_literals)?;
            let factor_right = build_tree(y, params, locals, seen_literals)?;
            Some(Tree::Fnmsub { factor_left: Box::new(factor_left), factor_right: Box::new(factor_right), base: Box::new(base) })
        }
        Expression::Binary { operator: BinaryOperator::Multiply, left, right } => {
            // A constant fmul factor is uncaptured (const-in-A evidence
            // exists only for fmadd); constant folding likewise.
            if matches!(left.as_ref(), Expression::FloatLiteral(_)) || matches!(right.as_ref(), Expression::FloatLiteral(_)) {
                return None;
            }
            let is_mul = |side: &Expression| matches!(side, Expression::Binary { operator: BinaryOperator::Multiply, .. });
            match (is_mul(left), is_mul(right)) {
                (false, false) => {
                    let left = build_tree(left, params, locals, seen_literals)?;
                    let right = build_tree(right, params, locals, seen_literals)?;
                    Some(Tree::Mul { left: Box::new(left), right: Box::new(right) })
                }
                // The SHALLOW mul-of-mul (measured both source orders emit
                // identically): one factor a leaf param product, the other a
                // single contracted madd — canonicalized chain-left. The
                // deeper chain breaks the register model (the cross-chain
                // product spans the window; float_mul_of_mul_deep) — defer.
                (true, false) | (false, true) => {
                    let (product, chain) = if is_mul(left) { (left, right) } else { (left, right) };
                    let (product, chain) = if is_mul(product.as_ref()) { (product, chain) } else { (chain, product) };
                    let Expression::Binary { operator: BinaryOperator::Multiply, left: x, right: y } = product.as_ref() else {
                        return None;
                    };
                    if !matches!(x.as_ref(), Expression::Variable(_)) || !matches!(y.as_ref(), Expression::Variable(_)) {
                        return None;
                    }
                    let chain_tree = build_tree(chain, params, locals, seen_literals)?;
                    if !matches!(chain_tree, Tree::Madd { .. } | Tree::Fnmsub { .. }) {
                        return None;
                    }
                    let x = build_tree(x, params, locals, seen_literals)?;
                    let y = build_tree(y, params, locals, seen_literals)?;
                    let product_tree = Tree::Mul { left: Box::new(x), right: Box::new(y) };
                    Some(Tree::Mul { left: Box::new(chain_tree), right: Box::new(product_tree) })
                }
                (true, true) => None,
            }
        }
        _ => None,
    }
}

/// Build an fmadd from `x*y + addend`, deferring constant-foldable pairs.
fn make_madd(
    x: &Expression,
    y: &Expression,
    addend: &Expression,
    params: &[(String, u32)],
    locals: &[(String, usize)],
    seen_literals: &mut Vec<u64>,
) -> Option<Tree> {
    let both_const = matches!(x, Expression::FloatLiteral(_)) && matches!(y, Expression::FloatLiteral(_));
    if both_const {
        return None;
    }
    let factor_left = build_tree(x, params, locals, seen_literals)?;
    let factor_right = build_tree(y, params, locals, seen_literals)?;
    let addend = build_tree(addend, params, locals, seen_literals)?;
    Some(Tree::Madd { factor_left: Box::new(factor_left), factor_right: Box::new(factor_right), addend: Box::new(addend) })
}

/// Count arith nodes in a subtree (the shallow mul-of-mul gate).
fn count_arith(tree: &Tree) -> usize {
    let mut refs: Vec<(&Tree, u32)> = Vec::new();
    collect_arith(tree, 0, &mut refs);
    refs.len()
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
        Tree::Madd { factor_left, factor_right, addend }
        | Tree::Fnmsub { factor_left, factor_right, base: addend }
        | Tree::Fmsub { factor_left, factor_right, subtrahend: addend } => {
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
        Tree::Param(_) | Tree::LocalRef(_) | Tree::Const(_) => {}
    }
}
