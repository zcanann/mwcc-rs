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
use mwcc_vreg::{assign_float_registers, linearize, DagNode, OpKind, FROZEN_FLOAT_REG, HAZARD_FPU};
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
    /// A plain unfused add (measured: a pooled constant + a non-mul value,
    /// the constant in the A slot — fadd f1,f0,f1).
    Fadd { left: Box<Tree>, right: Box<Tree> },
    /// A plain unfused subtract (neither side a product): source slots
    /// (fsub f1,f1,f0 — the k_sin else-tail's outer x-minus).
    Fsub { left: Box<Tree>, right: Box<Tree> },
}

/// One emitted node, operands in the final instruction slots (the measured
/// convention: a CONSTANT factor takes the A slot, otherwise source order).
enum FloatOp {
    Const(u64),
    /// x reloaded from its frame slot (no relocation).
    FrameLoad(i16),
    /// A conditionally-defined local (the diamond's qx): allocates like a
    /// window-top tier local but emits NOTHING — the diamond arms already
    /// loaded it.
    Phantom,
    Madd { a: Operand, c: Operand, b: Operand },
    Fnmsub { a: Operand, c: Operand, b: Operand },
    Fmsub { a: Operand, c: Operand, b: Operand },
    Mul { a: Operand, c: Operand },
    Add { a: Operand, b: Operand },
    Sub { a: Operand, b: Operand },
    /// The dual arm's liveness sink (emits nothing).
    Sink,
}

const LOAD_LATENCY: u32 = 2;
const FLOAT_ARITH_LATENCY: u32 = 3;
/// Double fmul GATES its consumers at 4 cycles while weighing 3 for
/// priority (measured: the z=x*x chains; see the linearize fixtures).
const FLOAT_MUL_GATE: u32 = 4;

impl Generator {
    /// Claim `return <double multiply-add tree>;` for the frozen float
    /// models. Returns whether it emitted the body (the caller appends the
    /// epilogue/blr).
    pub(crate) fn try_float_dag_return(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type != Type::Double
            || !function.statements.is_empty()
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        // GUARDS compose as `cmpwi; bXXlr` ahead of the float tail
        // (measured: the value must be the double param ALREADY in f1 — a
        // different float would need fmr + a real branch — and the condition
        // an int-param leaf compare).
        for guard in &function.guards {
            let Expression::Variable(value_name) = &guard.value else {
                return Ok(false);
            };
            let value_in_f1 = self
                .locations
                .get(value_name)
                .is_some_and(|location| location.class == ValueClass::Float && location.register == 1);
            if !value_in_f1 {
                return Ok(false);
            }
            let condition_param_ok = |name: &String| {
                self.locations
                    .get(name)
                    .is_some_and(|location| location.class == ValueClass::General)
            };
            let condition_ok = match &guard.condition {
                Expression::Variable(name) => condition_param_ok(name),
                Expression::Binary { operator, left, right } => {
                    matches!(
                        operator,
                        BinaryOperator::Less
                            | BinaryOperator::LessEqual
                            | BinaryOperator::Greater
                            | BinaryOperator::GreaterEqual
                            | BinaryOperator::Equal
                            | BinaryOperator::NotEqual
                    ) && matches!(left.as_ref(), Expression::Variable(name) if condition_param_ok(name))
                        && matches!(right.as_ref(), Expression::IntegerLiteral(value) if i16::try_from(*value).is_ok())
                }
                _ => false,
            };
            if !condition_ok {
                return Ok(false);
            }
        }
        // Named double locals are WINDOW-TOP tier values (measured: k_sin's
        // z/v take the top FPRs descending): each must be a plain scalar
        // double with an initializer.
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
        // Fold LOAD-DEPENDENT single-use locals into their use site
        // (measured: k_sin's r chain has no tier home — its registers flow
        // with the expression), keeping pure register-product locals (z, v)
        // as window-top-tier nodes. A multi-use load-dependent local is
        // uncaptured — defer.
        fn contains_literal(expression: &Expression) -> bool {
            match expression {
                Expression::FloatLiteral(_) => true,
                Expression::Binary { left, right, .. } => contains_literal(left) || contains_literal(right),
                _ => false,
            }
        }
        let mut fold_map: std::collections::HashMap<String, Expression> = std::collections::HashMap::new();
        let mut kept_locals: Vec<(String, Expression)> = Vec::new();
        for (index, local) in function.locals.iter().enumerate() {
            let Some(initializer) = local.initializer.as_ref() else {
                return Ok(false);
            };
            let resolved = crate::value_tracking::substitute(initializer, &fold_map);
            let later_uses: usize = function.locals[index + 1..]
                .iter()
                .filter_map(|later| later.initializer.as_ref())
                .map(|later| crate::analysis::count_name_occurrences(later, &local.name))
                .sum::<usize>()
                + crate::analysis::count_name_occurrences(return_expression, &local.name);
            if contains_literal(&resolved) {
                if later_uses != 1 {
                    return Ok(false);
                }
                fold_map.insert(local.name.clone(), resolved);
            } else {
                kept_locals.push((local.name.clone(), resolved));
            }
        }
        let folded_return = crate::value_tracking::substitute(return_expression, &fold_map);
        let return_expression = &folded_return;
        // Double parameters join the float DAG with their FPRs; int
        // (general-class) parameters are allowed alongside — they exist for
        // the guard conditions and never enter the DAG.
        let mut params: Vec<(u32, u8)> = Vec::new();
        let mut param_ids: Vec<(String, u32)> = Vec::new();
        let reload_mode = self.float_reload_x.is_some();
        for (name, register) in &self.float_pseudo_params {
            let value = (params.len() + 1) as u32;
            params.push((value, *register));
            param_ids.push((name.clone(), value));
        }
        for (index, parameter) in function.parameters.iter().enumerate() {
            let Some(location) = self.locations.get(&parameter.name) else {
                return Ok(false);
            };
            match parameter.parameter_type {
                Type::Double => {
                    if location.class != ValueClass::Float || location.width != 64 {
                        return Ok(false);
                    }
                    if reload_mode && index == 0 {
                        // x maps to the reload node (value id 9); its f1 is
                        // NOT a live param in the tail.
                        param_ids.push((parameter.name.clone(), 9));
                        continue;
                    }
                    let value = (params.len() + 1) as u32;
                    params.push((value, location.register));
                    param_ids.push((parameter.name.clone(), value));
                }
                _ if location.class == ValueClass::General => {}
                _ => return Ok(false),
            }
        }
        if let Some(name) = &self.float_phantom_local {
            // The diamond-defined local reads as value id 8 (the phantom
            // node's write), the same convention as the reload's 9.
            param_ids.push((name.clone(), 8));
        }
        if let Some((name, _)) = &self.float_frame_local {
            // The frame-resident diamond local reads as value id 7 — a
            // FrameLoad node in the tail's DAG.
            param_ids.push((name.clone(), 7));
        }
        // Lower the expression to the contracted tree (or bail).
        let mut seen_literals: Vec<u64> = Vec::new();
        let local_names: Vec<(String, usize)> = kept_locals
            .iter()
            .enumerate()
            .map(|(index, (name, _))| (name.clone(), index))
            .collect();
        // Each KEPT local's initializer must lower to exactly ONE arith node
        // over params and PRIOR locals (it is a pure register product — the
        // load-dependent ones folded above).
        let mut local_trees: Vec<Tree> = Vec::new();
        for (index, (_, initializer)) in kept_locals.iter().enumerate() {
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
        // A FRAME-resident diamond local as a ROOT-multiply factor blocks the
        // root contraction (measured: fmadd in register form, fmul+fadd/fsub
        // in frame form; the INNER contractions keep fusing, and qx as the
        // ADDEND fuses normally).
        fn subtree_reads_value(tree: &Tree, value: u32) -> bool {
            match tree {
                Tree::Param(read) => *read == value,
                Tree::Madd { factor_left, factor_right, addend }
                | Tree::Fnmsub { factor_left, factor_right, base: addend }
                | Tree::Fmsub { factor_left, factor_right, subtrahend: addend } => {
                    subtree_reads_value(factor_left, value)
                        || subtree_reads_value(factor_right, value)
                        || subtree_reads_value(addend, value)
                }
                Tree::Mul { left, right } | Tree::Fadd { left, right } | Tree::Fsub { left, right } => {
                    subtree_reads_value(left, value) || subtree_reads_value(right, value)
                }
                _ => false,
            }
        }
        let tree = if self.float_frame_local.is_some() {
            let direct = |side: &Tree| matches!(side, Tree::Param(7));
            let inside = |side: &Tree| subtree_reads_value(side, 7) && !matches!(side, Tree::Param(7));
            match tree {
                Tree::Madd { factor_left, factor_right, addend } => {
                    if !(direct(&factor_left) || direct(&factor_right))
                        && (inside(&factor_left) || inside(&factor_right))
                    {
                        return Ok(false);
                    }
                    if direct(&factor_left) || direct(&factor_right) {
                        if matches!(addend.as_ref(), Tree::Const(_)) {
                            // A pooled-constant addend on the decontracted
                            // root is unmeasured (probed DIFF) — defer.
                            return Ok(false);
                        }
                        self.output.anonymous_label_bump += 1;
                        Tree::Fadd {
                            left: addend,
                            right: Box::new(Tree::Mul { left: factor_left, right: factor_right }),
                        }
                    } else {
                        Tree::Madd { factor_left, factor_right, addend }
                    }
                }
                Tree::Fnmsub { factor_left, factor_right, base } => {
                    if !(direct(&factor_left) || direct(&factor_right))
                        && (inside(&factor_left) || inside(&factor_right))
                    {
                        return Ok(false);
                    }
                    if direct(&factor_left) || direct(&factor_right) {
                        self.output.anonymous_label_bump += 1;
                        Tree::Fsub {
                            left: base,
                            right: Box::new(Tree::Mul { left: factor_left, right: factor_right }),
                        }
                    } else {
                        Tree::Fnmsub { factor_left, factor_right, base }
                    }
                }
                Tree::Fmsub { factor_left, factor_right, subtrahend } => {
                    if subtree_reads_value(&factor_left, 7) || subtree_reads_value(&factor_right, 7) {
                        // qx*(...) - b in frame form is unmeasured.
                        return Ok(false);
                    }
                    Tree::Fmsub { factor_left, factor_right, subtrahend }
                }
                other => other,
            }
        } else {
            tree
        };
        // Local shapes beyond the captured range defer: a >= 4-arith return
        // tree over locals diverges (probed: the 4-coefficient z-chain), and
        // a local consumed ONLY by the return root alongside a >= 2-arith
        // chain is held back by mwcc's scheduler (the v=z*x fmul waits past
        // the chain's first fmadd — a far-consumer stall the order model
        // does not fit yet).
        if !kept_locals.is_empty() {
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
            for (index, _) in kept_locals.iter().enumerate() {
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
                let _ = return_arith;
                let _ = root_only;
                let _ = later_local_uses;
            }
        }
        // Order the arith nodes by (tree level DESC, factor-side first) and
        // group each node's constant loads before the arith block — the
        // measured construction the frozen linearizer was fitted against.
        let mut arith_refs: Vec<(&Tree, u32)> = Vec::new();
        collect_arith(&tree, 0, &mut arith_refs);
        if arith_refs.is_empty()
            || (arith_refs.len() < 2
                && seen_literals.is_empty()
                && self.float_phantom_local.is_none()
                && self.float_pseudo_params.is_empty())
        {
            // A bare constant return and const-free single ops stay on the
            // existing verified paths; a pooled-constant single op is ours —
            // and a PHANTOM or PSEUDO-PARAM tail (the dual arm's) has no
            // other path at any arity.
            return Ok(false);
        }
        arith_refs.sort_by_key(|&(_, level)| std::cmp::Reverse(level));

        let mut nodes: Vec<DagNode> = Vec::new();
        let mut ops: Vec<FloatOp> = Vec::new();
        // Map from each arith Tree's address to its node operand.
        let mut built: Vec<(*const Tree, Operand)> = Vec::new();
        // The fctiwz composition: x lives in the FRAME — its references
        // become a reload node (lfd from r1) and f1 frees for the chain.
        let mut reload_operand: Option<Operand> = None;
        if let Some(offset) = self.float_reload_x {
            let index = nodes.len();
            nodes.push(DagNode::new("lfd_x", LOAD_LATENCY).writes(&[9]));
            ops.push(FloatOp::FrameLoad(offset));
            reload_operand = Some(Operand::Node(index));
            let _ = reload_operand;
        }
        let mut phantom_operand: Option<Operand> = None;
        let mut phantom_index: Option<usize> = None;
        if self.float_phantom_local.is_some() {
            let index = nodes.len();
            nodes.push(DagNode::new("phantom", 0).local_home().writes(&[8]));
            ops.push(FloatOp::Phantom);
            phantom_operand = Some(Operand::Node(index));
            phantom_index = Some(index);
        }
        let mut frame_local_operand: Option<Operand> = None;
        if let Some((_, offset)) = self.float_frame_local {
            let index = nodes.len();
            nodes.push(DagNode::new("lfd_local", LOAD_LATENCY).writes(&[7]));
            ops.push(FloatOp::FrameLoad(offset));
            frame_local_operand = Some(Operand::Node(index));
        }
        // The locals' shared nodes, in declaration order (their arith emits
        // ahead of the loads: measured, z's fmul is slot 0).
        let mut local_operands: Vec<Operand> = Vec::new();
        for local_tree in &local_trees {
            let resolve_leaf = |leaf: &Tree| -> Option<Operand> {
                match leaf {
                    Tree::Param(9) => reload_operand,
                    Tree::Param(8) if phantom_operand.is_some() => phantom_operand,
                    Tree::Param(7) if frame_local_operand.is_some() => frame_local_operand,
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
            let mut node = DagNode::new("flocal", FLOAT_ARITH_LATENCY)
                .hazard(HAZARD_FPU)
                .local_home()
                .reads(&read_values)
                .writes(&[10 + index as u32]);
            if matches!(op, FloatOp::Mul { .. }) {
                node = node.gate(FLOAT_MUL_GATE);
            }
            nodes.push(node);
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
                Tree::Mul { left, right } | Tree::Fadd { left, right } | Tree::Fsub { left, right } => {
                    for side in [left, right] {
                        if let Tree::Const(bits) = side.as_ref() {
                            push_const(*bits, &mut nodes, &mut ops, &mut built, side.as_ref() as *const Tree);
                        }
                    }
                }
                _ => unreachable!("collect_arith yields arith nodes only"),
            }
        }
        // Pass 2: the arith nodes themselves (deepest level first).
        for &(arith, _) in &arith_refs {
            let resolve = |subtree: &Tree, built: &[(*const Tree, Operand)]| -> Option<Operand> {
                match subtree {
                    Tree::Param(9) => reload_operand,
                    Tree::Param(8) if phantom_operand.is_some() => phantom_operand,
                    Tree::Param(7) if frame_local_operand.is_some() => frame_local_operand,
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
                    let left = resolve(factor_left, &built).ok_or_else(|| Diagnostic::error("float DAG operand resolution"))?;
                    let right = resolve(factor_right, &built).ok_or_else(|| Diagnostic::error("float DAG operand resolution"))?;
                    let b = resolve(subtrahend, &built).ok_or_else(|| Diagnostic::error("float DAG operand resolution"))?;
                    // Measured: a pooled-constant factor takes A; else the
                    // source-left factor keeps it.
                    let (a, c) = if matches!(factor_right.as_ref(), Tree::Const(_)) { (right, left) } else { (left, right) };
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
                            .gate(FLOAT_MUL_GATE)
                            .hazard(HAZARD_FPU)
                            .reads(&reads)
                            .writes(&[10 + index as u32]),
                    );
                    ops.push(FloatOp::Mul { a, c });
                }
                Tree::Fadd { left, right } => {
                    let a = resolve(left, &built).ok_or_else(|| Diagnostic::error("float DAG operand resolution"))?;
                    let b = resolve(right, &built).ok_or_else(|| Diagnostic::error("float DAG operand resolution"))?;
                    let reads: Vec<u32> = [a, b].iter().map(|&operand| value_of(operand, &nodes)).collect();
                    nodes.push(
                        DagNode::new("fadd", FLOAT_ARITH_LATENCY)
                            .hazard(HAZARD_FPU)
                            .reads(&reads)
                            .writes(&[10 + index as u32]),
                    );
                    ops.push(FloatOp::Add { a, b });
                }
                Tree::Fsub { left, right } => {
                    let a = resolve(left, &built).ok_or_else(|| Diagnostic::error("float DAG operand resolution"))?;
                    let b = resolve(right, &built).ok_or_else(|| Diagnostic::error("float DAG operand resolution"))?;
                    let reads: Vec<u32> = [a, b].iter().map(|&operand| value_of(operand, &nodes)).collect();
                    let mut node = DagNode::new("fsub", FLOAT_ARITH_LATENCY)
                        .hazard(HAZARD_FPU)
                        .reads(&reads)
                        .writes(&[10 + index as u32]);
                    // The FSUB-rooted accumulator shape switches the register
                    // machine to the emission-order regime.
                    if std::ptr::eq(arith as *const Tree, &tree as *const Tree) {
                        node = node.emission_ordered();
                    }
                    nodes.push(node);
                    ops.push(FloatOp::Sub { a, b });
                }
                _ => unreachable!(),
            }
            built.push((arith as *const Tree, Operand::Node(index)));
        }

        let order = linearize(&nodes);
        let registers = assign_float_registers(&nodes, &order, &params, FROZEN_FLOAT_REG);
        if let Some(index) = phantom_index {
            self.float_phantom_register = registers[index];
        }
        if registers.iter().any(|register| register.is_none()) {
            {
            }
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
        // The guards sit AFTER the local-init prefix (measured: the z fmul
        // hoists above the cmpwi) and before the loads/chain. A local
        // scheduled past any non-local (the v shapes) is uncaptured with
        // guards — defer.
        let split = order
            .iter()
            .rposition(|&node| nodes[node].local_home)
            .map(|position| position + 1)
            .unwrap_or(0);
        if !function.guards.is_empty() && order[..split].iter().any(|&node| !nodes[node].local_home) {
            return Ok(false);
        }
        let mut emitted = 0usize;
        for &node in &order {
            if emitted == split {
                for guard in &function.guards {
                    let (options, condition_bit) = self.emit_condition_test(&guard.condition)?;
                    // The TRUE-condition conditional return (bnelr/bltlr...):
                    // invert the skip-branch encoding.
                    self.output.instructions.push(Instruction::BranchConditionalToLinkRegister {
                        options: options ^ 8,
                        condition_bit,
                    });
                    // The folded if's branch labels advance @N by 2 ahead of
                    // the pooled constants.
                    self.output.anonymous_label_bump += 2;
                }
            }
            emitted += 1;
            let d = registers[node].expect("checked above");
            match &ops[node] {
                FloatOp::Const(bits) => self.load_double_constant(d, *bits),
                FloatOp::FrameLoad(offset) => {
                    self.output.instructions.push(Instruction::LoadFloatDouble { d, a: 1, offset: *offset })
                }
                FloatOp::Phantom => {}
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
                    // order (fmul f0,f1,f2 for z*w); a pooled-CONSTANT factor
                    // keeps A (fmul f1,f0,f1); otherwise any VALUE operand
                    // sorts the registers DESCENDING into A.
                    let (mut ra, mut rc) = (register_of(*a), register_of(*c));
                    let both_params = matches!(a, Operand::Param(_)) && matches!(c, Operand::Param(_));
                    let const_a = matches!(a, Operand::Node(index) if matches!(ops[*index], FloatOp::Const(_)));
                    if !both_params && !const_a && rc > ra {
                        std::mem::swap(&mut ra, &mut rc);
                    }
                    self.output.instructions.push(Instruction::FloatMultiplyDouble { d, a: ra, c: rc });
                }
                FloatOp::Add { a, b } => self.output.instructions.push(Instruction::FloatAddDouble {
                    d,
                    a: register_of(*a),
                    b: register_of(*b),
                }),
                FloatOp::Sub { a, b } => self.output.instructions.push(Instruction::FloatSubtractDouble {
                    d,
                    a: register_of(*a),
                    b: register_of(*b),
                }),
                FloatOp::Sink => {}
            }
        }
        Ok(true)
    }
}

/// Lower an expression to the contracted tree. `None` defers: anything
/// outside the captured vocabulary (params + distinct double literals
/// combined by fmadd/fmul).
/// The destination of a float-family instruction (the dry-run claim
/// harvest for the escaping-root rule).
fn float_def(instruction: &Instruction) -> Option<u8> {
    match instruction {
        Instruction::LoadFloatDouble { d, .. }
        | Instruction::FloatMultiplyDouble { d, .. }
        | Instruction::FloatAddDouble { d, .. }
        | Instruction::FloatSubtractDouble { d, .. }
        | Instruction::FloatMultiplyAddDouble { d, .. }
        | Instruction::FloatMultiplySubtractDouble { d, .. }
        | Instruction::FloatNegativeMultiplySubtractDouble { d, .. } => Some(*d),
        _ => None,
    }
}

fn float_reads_register(instruction: &Instruction, register: u8) -> bool {
    match instruction {
        Instruction::FloatMultiplyDouble { a, c, .. } => *a == register || *c == register,
        Instruction::FloatAddDouble { a, b, .. } | Instruction::FloatSubtractDouble { a, b, .. } => {
            *a == register || *b == register
        }
        Instruction::FloatMultiplyAddDouble { a, c, b, .. }
        | Instruction::FloatMultiplySubtractDouble { a, c, b, .. }
        | Instruction::FloatNegativeMultiplySubtractDouble { a, c, b, .. } => {
            *a == register || *c == register || *b == register
        }
        _ => false,
    }
}

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
                (false, false) => {
                    // A plain fadd: one pooled-constant side (canonical A),
                    // the other a claimable non-mul value.
                    let (constant, other) = if matches!(left.as_ref(), Expression::FloatLiteral(_)) {
                        (left, right)
                    } else if matches!(right.as_ref(), Expression::FloatLiteral(_)) {
                        (right, left)
                    } else {
                        return None;
                    };
                    let constant = build_tree(constant, params, locals, seen_literals)?;
                    let other = build_tree(other, params, locals, seen_literals)?;
                    Some(Tree::Fadd { left: Box::new(constant), right: Box::new(other) })
                }
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
                // ONE pooled-constant factor takes the A slot (measured:
                // fmsub f0,f4,f2,f0 = 0.5*y - v*r); both fold — defer.
                if matches!(x.as_ref(), Expression::FloatLiteral(_)) && matches!(y.as_ref(), Expression::FloatLiteral(_)) {
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
                // Neither side a product: the plain unfused FSUB in source
                // slots (the deep form runs the emission-order regime).
                let minuend = build_tree(left, params, locals, seen_literals)?;
                let subtrahend = build_tree(right, params, locals, seen_literals)?;
                return Some(Tree::Fsub { left: Box::new(minuend), right: Box::new(subtrahend) });
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
            // ONE pooled-constant fmul factor is measured (fmul f1,f0,f1 —
            // the constant in A); both constant folds and stays deferred.
            let left_const = matches!(left.as_ref(), Expression::FloatLiteral(_));
            let right_const = matches!(right.as_ref(), Expression::FloatLiteral(_));
            if left_const && right_const {
                return None;
            }
            if left_const || right_const {
                let (constant, other) = if left_const { (left, right) } else { (right, left) };
                if matches!(other.as_ref(), Expression::Binary { operator: BinaryOperator::Multiply, .. }) {
                    return None;
                }
                let constant = build_tree(constant, params, locals, seen_literals)?;
                let other = build_tree(other, params, locals, seen_literals)?;
                return Some(Tree::Mul { left: Box::new(constant), right: Box::new(other) });
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
        Tree::Mul { left, right } | Tree::Fadd { left, right } | Tree::Fsub { left, right } => {
            into.push((tree, level));
            collect_arith(left, level + 1, into);
            collect_arith(right, level + 1, into);
        }
        Tree::Param(_) | Tree::LocalRef(_) | Tree::Const(_) => {}
    }
}

impl Generator {
    /// The PUNNED-BITS guard + float-DAG composition (the k_sin prefix):
    /// `int ix = *(int*)&x [& 0x7fffffff]; if (ix < C) return x; <float tail>`
    /// emits the measured frame form — stwu -16; [lis r0 staged FIRST for a
    /// lis-able C]; stfd f1,8(r1); lwz; [clrlwi ,1]; cmpw/cmpwi; bge +8;
    /// b EPILOGUE — extra int guards in branch form, the float tail, then
    /// the SHARED addi/blr epilogue.
    pub(crate) fn try_punned_guard_float_return(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        // The NESTED inner guard (k_sin): `if (ix < C) { if ((int)x == 0)
        // return x; }` arrives as one statement — followed by the C89 local
        // assigns the leading normalizer could not reach past the if. The
        // flat form arrives as a hoisted guard. Exactly one of the two.
        // A trailing iy-split (one non-x guard + else-return) composes with
        // the nested form: the guard passes through to the DUAL tail arm.
        let dual_tail = function.guards.len() == 1
            && function.return_expression.is_some()
            && !matches!(&function.guards[0].value, Expression::Variable(_));
        // The nested prefix accepts: no trailing control (plain), a
        // normalized guard dual, or the k_cos IF-FORM (guards empty, no
        // return expression, the last statement an if/else with bodies).
        let nested = matches!(
            function.statements.first(),
            Some(Statement::If { .. }) if function.guards.is_empty() || dual_tail
        );
        // Trailing `z = x*x;`-style assigns behind the nested if become
        // initializers (each target a declared, uninitialized local,
        // assigned once).
        let mut trailing_inits: Vec<(String, Expression)> = Vec::new();
        // The k_cos IF-FORM, as the parser FLATTENS it (the else of a
        // returning-then becomes fall-through):
        //   If(prefix), assigns..., If{cond, then:[Return T], else:[]},
        //   If(diamond), assigns..., Return(E)
        let if_form_start: Option<usize> = if nested && function.guards.is_empty() && function.return_expression.is_none() {
            let statements = &function.statements;
            (1..statements.len().saturating_sub(2))
                .find(|&index| {
                    matches!(
                        &statements[index],
                        Statement::If { then_body, else_body, .. }
                            if matches!(then_body.as_slice(), [Statement::Return(Some(_))])
                                && else_body.is_empty()
                    ) && matches!(&statements[index + 1], Statement::If { .. })
                        && matches!(statements.last(), Some(Statement::Return(Some(_))))
                        && statements[1..index].iter().all(|s| matches!(s, Statement::Assign { .. }))
                        && statements[index + 2..statements.len() - 1]
                            .iter()
                            .all(|s| matches!(s, Statement::Assign { .. }))
                })
        } else {
            None
        };
        if nested {
            let trailing_end = if_form_start.unwrap_or(function.statements.len());
            for statement in &function.statements[1..trailing_end] {
                let Statement::Assign { name, value } = statement else {
                    return Ok(false);
                };
                let declared_uninit = function
                    .locals
                    .iter()
                    .any(|local| &local.name == name && local.initializer.is_none() && local.array_length.is_none());
                if !declared_uninit || trailing_inits.iter().any(|(seen, _)| seen == name) {
                    return Ok(false);
                }
                trailing_inits.push((name.clone(), value.clone()));
            }
        }
        if function.return_type != Type::Double
            || function.locals.is_empty()
            || (!nested && (!function.statements.is_empty() || function.guards.is_empty()))
        {
            return Ok(false);
        }
        // Decompose the if-form: the dual condition + then value, the inner
        // diamond, the else-only fold locals, and the else return.
        struct IfForm<'a> {
            condition: &'a Expression,
            then_value: &'a Expression,
            diamond: &'a Statement,
            else_assigns: Vec<(&'a String, &'a Expression)>,
            else_return: &'a Expression,
        }
        let if_form: Option<IfForm> = match if_form_start {
            None => None,
            Some(start) => {
                let statements = &function.statements;
                let Statement::If { condition, then_body, .. } = &statements[start] else {
                    unreachable!("matched above");
                };
                let [Statement::Return(Some(then_value))] = then_body.as_slice() else {
                    return Ok(false);
                };
                let diamond = &statements[start + 1];
                let Some(Statement::Return(Some(else_return))) = statements.last() else {
                    return Ok(false);
                };
                let mut else_assigns: Vec<(&String, &Expression)> = Vec::new();
                for statement in &statements[start + 2..statements.len() - 1] {
                    let Statement::Assign { name, value } = statement else {
                        return Ok(false);
                    };
                    else_assigns.push((name, value));
                }
                Some(IfForm { condition, then_value, diamond, else_assigns, else_return })
            }
        };
        let _ = &trailing_inits;
        let Some(first_param) = function.parameters.first() else {
            return Ok(false);
        };
        if first_param.parameter_type != Type::Double {
            return Ok(false);
        }
        let x = first_param.name.as_str();
        // locals[0] = the punned int read of x's high word.
        let ix_local = &function.locals[0];
        if ix_local.declared_type != Type::Int || ix_local.array_length.is_some() {
            return Ok(false);
        }
        let Some(ix_init) = ix_local.initializer.as_ref() else {
            return Ok(false);
        };
        let (pun, masked) = match crate::frame::pun_word_offset_pub(ix_init, x) {
            Some(0) => (true, false),
            _ => match ix_init {
                Expression::Binary { operator: BinaryOperator::BitAnd, left, right } => {
                    let mask31 = |side: &Expression| crate::analysis::constant_value(side) == Some(0x7fff_ffff);
                    if crate::frame::pun_word_offset_pub(left, x) == Some(0) && mask31(right) {
                        (true, true)
                    } else if crate::frame::pun_word_offset_pub(right, x) == Some(0) && mask31(left) {
                        (true, true)
                    } else {
                        (false, false)
                    }
                }
                _ => (false, false),
            },
        };
        if !pun {
            return Ok(false);
        }
        let ix = ix_local.name.as_str();
        // The ix compare: `ix < C` from guards[0] (flat) or the outer nested
        // if; C either cmpwi-able or lis-able (low half zero). The nested
        // inner body must be exactly `if ((int)x == 0) return x;`.
        let mut early_return_const: Option<u64> = None;
        let outer_condition: &Expression = if nested {
            let Some(Statement::If { condition, then_body, else_body }) = function.statements.first() else {
                return Ok(false);
            };
            if !else_body.is_empty() {
                return Ok(false);
            }
            let [Statement::If { condition: inner, then_body: inner_then, else_body: inner_else }] = then_body.as_slice() else {
                return Ok(false);
            };
            if !inner_else.is_empty() {
                return Ok(false);
            }
            match inner_then.as_slice() {
                [Statement::Return(Some(Expression::Variable(name)))] if name == x => {}
                // `return one;` — a folded static-const double pools and
                // loads into f1 ahead of the epilogue branch (measured:
                // k_cos's early return).
                [Statement::Return(Some(Expression::FloatLiteral(value)))] => {
                    early_return_const = Some(value.to_bits());
                }
                _ => return Ok(false),
            }
            let Expression::Binary { operator: BinaryOperator::Equal, left, right } = inner else {
                return Ok(false);
            };
            let cast_of_x = matches!(left.as_ref(), Expression::Cast { operand, target_type: Type::Int }
                if matches!(operand.as_ref(), Expression::Variable(name) if name == x));
            if !cast_of_x || crate::analysis::constant_value(right) != Some(0) {
                return Ok(false);
            }
            condition
        } else {
            let first_guard = &function.guards[0];
            if !matches!(&first_guard.value, Expression::Variable(name) if name == x) {
                return Ok(false);
            }
            &first_guard.condition
        };
        let int_params_early = function
            .parameters
            .iter()
            .filter(|parameter| parameter.parameter_type != Type::Double)
            .count() as u8;
        let Expression::Binary { operator: BinaryOperator::Less, left, right } = outer_condition else {
            return Ok(false);
        };
        if !matches!(left.as_ref(), Expression::Variable(name) if name == ix) {
            return Ok(false);
        }
        let Some(compare_constant) = crate::analysis::constant_value(right) else {
            return Ok(false);
        };
        let small_compare = i16::try_from(compare_constant).ok();
        let lis_high: Option<i16> = (small_compare.is_none()
            && (compare_constant & 0xffff) == 0
            && u32::try_from(compare_constant).is_ok())
        .then(|| (compare_constant >> 16) as i16);
        if small_compare.is_none() && lis_high.is_none() {
            return Ok(false);
        }
        // The k_cos family: the trailing dual may split on the PRESERVED ix
        // (`if (ix < C) ... else ...`) — one leaf comparison against an i16
        // literal. ix then stays live in the prefix's target register.
        let effective_dual_condition: Option<&Expression> = if let Some(form) = &if_form {
            Some(form.condition)
        } else if nested && dual_tail {
            Some(&function.guards[0].condition)
        } else {
            None
        };
        let ix_in_dual_condition = nested
            && effective_dual_condition.is_some()
            && matches!(effective_dual_condition.expect("checked"),
                Expression::Binary { operator, left, right }
                    if matches!(operator, BinaryOperator::Less | BinaryOperator::LessEqual
                        | BinaryOperator::Greater | BinaryOperator::GreaterEqual
                        | BinaryOperator::Equal | BinaryOperator::NotEqual)
                        && matches!(left.as_ref(), Expression::Variable(name) if name == ix)
                        && matches!(right.as_ref(), Expression::IntegerLiteral(value) if i16::try_from(*value).is_ok()));
        // The k_cos BIG-constant split (`ix < 0x3FD33333`): the constant
        // materializes lis r3 + addi r0 INSIDE the shared schedule, so the
        // prefix must keep ix out of r3 — the SPLIT form (lwz r3 raw,
        // clrlwi r4). Measured for Less with a positive addi low half and
        // no int params.
        let ix_dual_big: Option<(i16, i16)> = if nested && effective_dual_condition.is_some() && !ix_in_dual_condition {
            match effective_dual_condition.expect("checked") {
                Expression::Binary { operator: BinaryOperator::Less, left, right }
                    if matches!(left.as_ref(), Expression::Variable(name) if name == ix) =>
                {
                    match right.as_ref() {
                        Expression::IntegerLiteral(value)
                            if u32::try_from(*value).is_ok()
                                && (*value & 0xffff) <= 0x7fff
                                && i16::try_from(*value).is_err() =>
                        {
                            Some(((*value >> 16) as i16, (*value & 0xffff) as i16))
                        }
                        _ => None,
                    }
                }
                _ => None,
            }
        } else {
            None
        };
        // ix appears nowhere else.
        let ix_uses_elsewhere = function
            .guards
            .iter()
            .enumerate()
            .filter(|&(index, _)| !(nested && index == 0 && (ix_in_dual_condition || ix_dual_big.is_some())))
            .map(|(_, guard)| guard)
            .skip(if nested { 0 } else { 1 })
            .map(|guard| {
                crate::analysis::count_name_occurrences(&guard.condition, ix)
                    + crate::analysis::count_name_occurrences(&guard.value, ix)
            })
            .sum::<usize>()
            + function
                .locals
                .iter()
                .skip(1)
                .filter_map(|local| local.initializer.as_ref())
                .map(|init| crate::analysis::count_name_occurrences(init, ix))
                .sum::<usize>()
            + function
                .return_expression
                .as_ref()
                .map(|ret| crate::analysis::count_name_occurrences(ret, ix))
                .unwrap_or(0);
        if ix_uses_elsewhere != 0 {
            return Ok(false);
        }
        // Extra guards: int-param leaf conditions returning x (branch form).
        let extra_guards = if nested { &function.guards[0..0] } else { &function.guards[1..] };
        for guard in extra_guards {
            if !matches!(&guard.value, Expression::Variable(name) if name == x) {
                return Ok(false);
            }
            let ok = match &guard.condition {
                Expression::Variable(name) => self
                    .locations
                    .get(name)
                    .is_some_and(|location| location.class == ValueClass::General),
                _ => false,
            };
            if !ok {
                return Ok(false);
            }
        }
        if ix_in_dual_condition && lis_high.is_none() {
            // The preserved-ix dual is measured only in the lis/cmpw form
            // (target r3/r4); the r0 small-compare form would be clobbered.
            return Ok(false);
        }
        if ix_dual_big.is_some() && (lis_high.is_none() || int_params_early != 0 || !masked) {
            return Ok(false);
        }
        // NOT YET SHIPPABLE: the composed ELSE TAIL (x re-reload + the
        // diamond frame local + folded hz/a) claims but its register/order
        // model is unfitted (probed fire 367: qx load f3 vs ours f5; the
        // x*y fmul keeps SOURCE order with a reload operand where ours
        // swaps register-DESC; r lands f4 vs f8) — the if-form defers until
        // the two-frame-load tail class is captured and fitted.
        if if_form.is_some() {
            return Ok(false);
        }
        // The k_cos ELSE COMPOSITION payload: the inner diamond + fold
        // locals, valid only in the big-const split mode (ix alive in r4,
        // the raw r3 free for the addis).
        let composition: Option<crate::generator::FloatElseComposition> = match &if_form {
            None => None,
            Some(form) => {
                if ix_dual_big.is_none() {
                    return Ok(false);
                }
                let Statement::If { condition: inner, then_body, else_body } = form.diamond else {
                    return Ok(false);
                };
                // `ix > BIG2` with a lis-able constant: lis r0 + cmpw + ble.
                let Expression::Binary { operator: BinaryOperator::Greater, left, right } = inner else {
                    return Ok(false);
                };
                if !matches!(left.as_ref(), Expression::Variable(name) if name == ix) {
                    return Ok(false);
                }
                let Some(inner_constant) = crate::analysis::constant_value(right) else {
                    return Ok(false);
                };
                if inner_constant & 0xffff != 0 || u32::try_from(inner_constant).is_err() {
                    return Ok(false);
                }
                // The diamond arms: qx = literal / punned ix-minus-C stores.
                let [Statement::Assign { name: qx_name, value: Expression::FloatLiteral(then_value) }] =
                    then_body.as_slice()
                else {
                    return Ok(false);
                };
                let qx_ok = function.locals.iter().any(|local| {
                    &local.name == qx_name
                        && local.declared_type == Type::Double
                        && local.initializer.is_none()
                        && local.array_length.is_none()
                });
                if !qx_ok {
                    return Ok(false);
                }
                let [Statement::Store { target: hi_target, value: hi_value }, Statement::Store { target: lo_target, value: lo_value }] =
                    else_body.as_slice()
                else {
                    return Ok(false);
                };
                if crate::frame::pun_word_offset_pub(hi_target, qx_name) != Some(0)
                    || crate::frame::pun_word_offset_pub(lo_target, qx_name) != Some(4)
                    || crate::analysis::constant_value(lo_value) != Some(0)
                {
                    return Ok(false);
                }
                let Expression::Binary { operator: BinaryOperator::Subtract, left: hi_left, right: hi_right } = hi_value
                else {
                    return Ok(false);
                };
                if !matches!(hi_left.as_ref(), Expression::Variable(name) if name == ix) {
                    return Ok(false);
                }
                let Some(subtracted) = crate::analysis::constant_value(hi_right) else {
                    return Ok(false);
                };
                if subtracted & 0xffff != 0 {
                    return Ok(false);
                }
                // The else-only fold locals (hz, a): declared, uninitialized.
                let mut else_locals: Vec<mwcc_syntax_trees::LocalDeclaration> = Vec::new();
                for (name, value) in &form.else_assigns {
                    let Some(declared) = function.locals.iter().find(|local| {
                        &&local.name == name
                            && local.declared_type == Type::Double
                            && local.initializer.is_none()
                            && local.array_length.is_none()
                    }) else {
                        return Ok(false);
                    };
                    let mut normalized = declared.clone();
                    normalized.initializer = Some((*value).clone());
                    else_locals.push(normalized);
                }
                Some(crate::generator::FloatElseComposition {
                    compare_high: (inner_constant >> 16) as i16,
                    skip_options: 4,
                    skip_bit: 1,
                    ix_register: 0, // filled at emission (target_register)
                    addis_target: 0,
                    then_bits: then_value.to_bits(),
                    addis_shift: ((-subtracted) >> 16) as i16,
                    qx_name: qx_name.clone(),
                    qx_offset: 16,
                    else_locals,
                })
            }
        };
        // The synthetic tail: the double locals + return, no guards; the
        // nested form's trailing assigns become initializers in ASSIGNMENT
        // order (the tier's definition order).
        let mut synthetic_locals: Vec<mwcc_syntax_trees::LocalDeclaration> = Vec::new();
        for (name, value) in &trailing_inits {
            let declared = function
                .locals
                .iter()
                .find(|local| &local.name == name)
                .expect("checked above");
            let mut normalized = declared.clone();
            normalized.initializer = Some(value.clone());
            synthetic_locals.push(normalized);
        }
        for local in &function.locals[1..] {
            if trailing_inits.iter().any(|(name, _)| name == &local.name) {
                continue;
            }
            // Composition: the diamond local and the else-only fold locals
            // stay OUT of the shared synthetic (the else tail owns them).
            if let Some(payload) = &composition {
                if local.name == payload.qx_name
                    || payload.else_locals.iter().any(|owned| owned.name == local.name)
                {
                    continue;
                }
            }
            synthetic_locals.push(local.clone());
        }
        let (synthetic_guards, synthetic_return) = if let Some(form) = &if_form {
            (
                vec![mwcc_syntax_trees::GuardedReturn {
                    condition: form.condition.clone(),
                    value: form.then_value.clone(),
                }],
                Some(form.else_return.clone()),
            )
        } else if nested {
            (function.guards.clone(), function.return_expression.clone())
        } else {
            (Vec::new(), function.return_expression.clone())
        };
        let synthetic = Function {
            return_type: function.return_type,
            name: function.name.clone(),
            is_static: function.is_static,
            is_weak: function.is_weak,
            parameters: function.parameters.clone(),
            locals: synthetic_locals,
            statements: Vec::new(),
            guards: synthetic_guards,
            return_expression: synthetic_return,
        };
        let _ = Statement::Return(None); // keep the use import stable

        // ---- emission (rollback on a tail decline) ----
        let instructions_before = self.output.instructions.len();
        let relocations_before = self.output.relocations.len();
        let bump_before = self.output.anonymous_label_bump;
        let frame_before = self.frame_size;
        // The frame drives the extab/extabindex sections; the nested fctiwz
        // form needs a second conversion slot.
        let frame_size: i16 = if nested { 32 } else { 16 };
        self.frame_size = frame_size;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -frame_size });
        if let Some(high) = lis_high {
            self.output.instructions.push(Instruction::load_immediate_shifted(0, high));
        }
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 8 });
        let int_params = function
            .parameters
            .iter()
            .filter(|parameter| parameter.parameter_type != Type::Double)
            .count() as u8;
        // The big-const dual SPLITS raw/masked (lwz r3; clrlwi r4,r3) so the
        // dual's lis can take r3; otherwise the mask is in-place.
        let load_register = if lis_high.is_some() { 3 + int_params } else { 0 };
        let target_register = if ix_dual_big.is_some() { load_register + 1 } else { load_register };
        self.output.instructions.push(Instruction::LoadWord { d: load_register, a: 1, offset: 8 });
        if masked {
            self.output.instructions.push(Instruction::ClearLeftImmediate { a: target_register, s: load_register, clear: 1 });
        }
        if lis_high.is_some() {
            self.output.instructions.push(Instruction::CompareWord { a: target_register, b: 0 });
        } else {
            self.output.instructions.push(Instruction::CompareWordImmediate {
                a: target_register,
                immediate: small_compare.expect("checked above"),
            });
        }
        let mut epilogue_branches: Vec<usize> = Vec::new();
        let mut tail_branches: Vec<usize> = Vec::new();
        if nested {
            // bge TAIL; fctiwz f0,f1; stfd f0,16; lwz r0,20; cmpwi; bne TAIL;
            // b EPILOGUE (measured).
            tail_branches.push(self.output.instructions.len());
            self.output.instructions.push(Instruction::BranchConditionalForward { options: 4, condition_bit: 0, target: 0 });
            self.output.instructions.push(Instruction::ConvertToIntegerWordZero { d: 0, b: 1 });
            let conversion_slot: i16 = if composition.is_some() { 24 } else { 16 };
            self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: conversion_slot });
            self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: conversion_slot + 4 });
            self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
            tail_branches.push(self.output.instructions.len());
            self.output.instructions.push(Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 0 });
            if let Some(bits) = early_return_const {
                self.load_double_constant(1, bits);
                // The const-return early path consumes ONE fewer pre-pool
                // label than return-x (measured: pool @10 vs @11).
                self.output.anonymous_label_bump -= 1;
            }
            epilogue_branches.push(self.output.instructions.len());
            self.output.instructions.push(Instruction::Branch { target: 0 });
            // Two folded ifs + the epilogue block; the fctiwz is an
            // int<->float conversion (its own pre-pool label).
            self.output.anonymous_label_bump += 4;
            self.output.has_conversion = true;
            // The inner block consumes one more number AFTER the pools.
            self.output.post_constant_label_bump += 1;
        } else {
            // bge +8 skips the epilogue branch (Less: skip on the inverse).
            let skip_index = self.output.instructions.len();
            self.output.instructions.push(Instruction::BranchConditionalForward { options: 4, condition_bit: 0, target: skip_index + 2 });
            epilogue_branches.push(self.output.instructions.len());
            self.output.instructions.push(Instruction::Branch { target: 0 });
            self.output.anonymous_label_bump += 2;
        }
        for guard in extra_guards {
            let (options, condition_bit) = self.emit_condition_test(&guard.condition)?;
            let skip_index = self.output.instructions.len();
            self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: skip_index + 2 });
            epilogue_branches.push(self.output.instructions.len());
            self.output.instructions.push(Instruction::Branch { target: 0 });
            self.output.anonymous_label_bump += 2;
        }
        // The SHARED epilogue block consumes ONE extra label ahead of the
        // pooled constants (measured: the one-guard shape pools at @8/@9
        // with 2 if-labels + this 1).
        self.output.anonymous_label_bump += 1;
        // Flat mode: the tail reads x from f1 (the spill stays valid).
        // Nested mode: x RELOADS from the frame (measured — f1 frees for
        // the chain).
        let tail_start = self.output.instructions.len();
        for branch in &tail_branches {
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[*branch] {
                *target = tail_start;
            }
        }
        let saved_frame_slots = std::mem::take(&mut self.frame_slots);
        if nested {
            self.float_reload_x = Some(8);
        }
        // The preserved ix resolves in the dual's condition test through a
        // temporary location at the prefix's compare register.
        if let Some((high, low)) = ix_dual_big {
            self.float_dual_compare = Some((high, low, target_register));
        }
        if let Some(mut payload) = composition {
            payload.ix_register = target_register;
            payload.addis_target = load_register;
            self.float_else_composition = Some(payload);
        }
        let saved_ix_location = if ix_in_dual_condition {
            self.locations.insert(
                ix.to_string(),
                crate::generator::Location {
                    class: ValueClass::General,
                    register: target_register,
                    signed: true,
                    width: 32,
                    pointee: None,
                    stride: None,
                },
            )
        } else {
            None
        };
        let claimed = if synthetic.guards.is_empty() {
            self.try_float_dag_return(&synthetic)
        } else {
            self.try_dual_tail_float_return(&synthetic)
        };
        if ix_in_dual_condition {
            match saved_ix_location {
                Some(previous) => {
                    self.locations.insert(ix.to_string(), previous);
                }
                None => {
                    self.locations.remove(ix);
                }
            }
        }
        self.float_reload_x = None;
        self.float_dual_compare = None;
        self.float_else_composition = None;
        self.frame_slots = saved_frame_slots;
        match claimed {
            Ok(true) => {}
            other => {
                self.output.instructions.truncate(instructions_before);
                self.output.relocations.truncate(relocations_before);
                self.output.anonymous_label_bump = bump_before;
                self.frame_size = frame_before;
                return other.map(|_| false);
            }
        }
        let epilogue = self.output.instructions.len();
        for branch in epilogue_branches {
            if let Instruction::Branch { target } = &mut self.output.instructions[branch] {
                *target = epilogue;
            }
        }
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: frame_size });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}

impl Generator {
    /// The DUAL-TAIL float return (the k_sin iy split's simple form):
    /// `if (<int cond>) return A; else return B;` — a leaf function, each
    /// tail an INDEPENDENT float DAG ending in its own blr (measured: both
    /// tails allocate with the full window; a bare-x tail flips to the bclr
    /// guard form upstream and never reaches here).
    pub(crate) fn try_dual_tail_float_return(&mut self, function: &Function) -> Compilation<bool> {
        // The parser normalizes `if (c) return A; else return B;` into a
        // guard {condition, value: A} + the trailing return B. Shared
        // locals — pure register PRODUCTS (z = x*x) and load-dependent
        // CHAINS (the k_sin r) — materialize as ONE shared DAG ahead of the
        // compare and feed both tails as pseudo-params. The register
        // machine runs in DUAL mode: prefix tier definition-descending, a
        // STORE sink carrying tail liveness, the tails' pressure as the
        // window floor, and escaping chain roots on their C-operand.
        if function.return_type != Type::Double
            || function.guards.len() != 1
            || !function.statements.is_empty()
            || function.return_expression.is_none()
        {
            return Ok(false);
        }
        let guard = &function.guards[0];
        let then_value = &guard.value;
        let else_value = function.return_expression.as_ref().expect("checked above");
        if matches!(then_value, Expression::Variable(_)) {
            return Ok(false);
        }
        let condition_ok = match &guard.condition {
            Expression::Variable(name) => self
                .locations
                .get(name)
                .is_some_and(|location| location.class == ValueClass::General),
            Expression::Binary { operator, left, right } => {
                matches!(
                    operator,
                    BinaryOperator::Less
                        | BinaryOperator::LessEqual
                        | BinaryOperator::Greater
                        | BinaryOperator::GreaterEqual
                        | BinaryOperator::Equal
                        | BinaryOperator::NotEqual
                ) && matches!(left.as_ref(), Expression::Variable(name)
                    if self.locations.get(name).is_some_and(|location| location.class == ValueClass::General))
                    && matches!(right.as_ref(), Expression::IntegerLiteral(value) if i16::try_from(*value).is_ok())
            }
            _ => false,
        };
        // The punned caller vets the BIG-const ix compare itself (the
        // lis/addi/cmpw weave) — condition_ok only gates the leaf forms.
        if !condition_ok && self.float_dual_compare.is_none() {
            return Ok(false);
        }
        // IN-FRAME (the punned k_sin composition): x lives in the frame —
        // its shared-DAG references become a reload node (value id 9, the
        // core arm's convention) and f1 frees for the chain.
        let in_frame = self.float_reload_x.is_some();
        // Double params for the shared DAG.
        let mut params: Vec<(u32, u8)> = Vec::new();
        let mut param_ids: Vec<(String, u32)> = Vec::new();
        for (index, parameter) in function.parameters.iter().enumerate() {
            let Some(location) = self.locations.get(&parameter.name) else {
                return Ok(false);
            };
            match parameter.parameter_type {
                Type::Double => {
                    if location.class != ValueClass::Float || location.width != 64 {
                        return Ok(false);
                    }
                    if in_frame && index == 0 {
                        param_ids.push((parameter.name.clone(), 9));
                        continue;
                    }
                    let value = (params.len() + 1) as u32;
                    params.push((value, location.register));
                    param_ids.push((parameter.name.clone(), value));
                }
                _ if location.class == ValueClass::General => {}
                _ => return Ok(false),
            }
        }
        // The shared locals as trees over params + prior locals.
        let mut local_names: Vec<(String, usize)> = Vec::new();
        let mut local_trees: Vec<Tree> = Vec::new();
        let mut shared_literals: Vec<u64> = Vec::new();
        for local in &function.locals {
            if local.declared_type != Type::Double || local.array_length.is_some() {
                return Ok(false);
            }
            let Some(init) = local.initializer.as_ref() else {
                return Ok(false);
            };
            let mut chain_literals: Vec<u64> = Vec::new();
            let Some(tree) = build_tree(init, &param_ids, &local_names, &mut chain_literals) else {
                return Ok(false);
            };
            // A literal duplicated across locals shares its load — unmodeled.
            if chain_literals.iter().any(|bits| shared_literals.contains(bits)) {
                return Ok(false);
            }
            shared_literals.extend(chain_literals);
            local_names.push((local.name.clone(), local_trees.len()));
            local_trees.push(tree);
        }
        // A literal in BOTH the shared region and a tail keeps the shared
        // load live into the tail (measured: the 0.5 chain/else dup DIFFs
        // if reloaded) — defer until that share is modeled.
        {
            let mut tail_literals: Vec<u64> = Vec::new();
            collect_literals(then_value, &mut tail_literals);
            collect_literals(else_value, &mut tail_literals);
            if tail_literals.iter().any(|bits| shared_literals.contains(bits)) {
                return Ok(false);
            }
        }
        // Tail liveness: which locals/params each tail reads.
        let reads_of = |tree_value: &Expression, name: &str| crate::analysis::count_name_occurrences(tree_value, name);
        let escaping_locals: Vec<usize> = (0..local_trees.len())
            .filter(|&index| {
                let name = &local_names[index].0;
                reads_of(then_value, name) + reads_of(else_value, name) > 0
            })
            .collect();
        if !function.locals.is_empty() && escaping_locals.is_empty() {
            return Ok(false);
        }
        // The tails' window pressure (the shared DAG cannot see it).
        let tail_pressure = |value: &Expression| -> u8 {
            let locals_read = (0..local_trees.len())
                .filter(|&index| reads_of(value, &local_names[index].0) > 0)
                .count();
            let params_read = param_ids
                .iter()
                .filter(|(name, _)| reads_of(value, name) > 0)
                .count();
            let mut literals: Vec<u64> = Vec::new();
            collect_literals(value, &mut literals);
            (locals_read + params_read + literals.len()) as u8
        };
        // UNION floor (measured, K=2 w=v*z matrix probe): locals read by
        // EITHER tail + params read by either tail + the max per-tail
        // literal count — not the per-tail max (K=2 wants 6, per-tail says 5).
        let union_floor = {
            let locals_read = (0..local_trees.len())
                .filter(|&index| {
                    let name = &local_names[index].0;
                    reads_of(then_value, name) + reads_of(else_value, name) > 0
                })
                .count();
            let params_read = param_ids
                .iter()
                .filter(|(name, _)| reads_of(then_value, name) + reads_of(else_value, name) > 0)
                .count();
            let mut then_literals: Vec<u64> = Vec::new();
            collect_literals(then_value, &mut then_literals);
            let mut else_literals: Vec<u64> = Vec::new();
            collect_literals(else_value, &mut else_literals);
            (locals_read + params_read + then_literals.len().max(else_literals.len())) as u8
        };
        let window_floor = tail_pressure(then_value).max(tail_pressure(else_value)).max(union_floor);

        // ---- the shared DAG ----
        let mut nodes: Vec<DagNode> = Vec::new();
        let mut ops: Vec<FloatOp> = Vec::new();
        let mut built: Vec<(*const Tree, Operand)> = Vec::new();
        let mut local_operands: Vec<Operand> = Vec::new();
        let mut local_is_product: Vec<bool> = Vec::new();
        let mut next_value = 40u32;
        if in_frame {
            nodes.push(DagNode::new("lfd_x", LOAD_LATENCY).writes(&[9]));
            ops.push(FloatOp::FrameLoad(self.float_reload_x.expect("in_frame")));
        }
        for local_tree in &local_trees {
            // Loads first (per chain), then its arith deepest-level first.
            let mut refs: Vec<(&Tree, u32)> = Vec::new();
            collect_arith(local_tree, 0, &mut refs);
            refs.sort_by_key(|&(_, level)| std::cmp::Reverse(level));
            for &(arith, _) in &refs {
                let mut push_const = |bits: u64, nodes: &mut Vec<DagNode>, ops: &mut Vec<FloatOp>, built: &mut Vec<(*const Tree, Operand)>, key: *const Tree, next_value: &mut u32| {
                    let index = nodes.len();
                    nodes.push(DagNode::new("lfd", LOAD_LATENCY).writes(&[*next_value]));
                    *next_value += 1;
                    ops.push(FloatOp::Const(bits));
                    built.push((key, Operand::Node(index)));
                };
                match arith {
                    Tree::Madd { factor_left, factor_right, addend }
                    | Tree::Fnmsub { factor_left, factor_right, base: addend }
                    | Tree::Fmsub { factor_left, factor_right, subtrahend: addend } => {
                        for side in [factor_left, factor_right, addend] {
                            if let Tree::Const(bits) = side.as_ref() {
                                push_const(*bits, &mut nodes, &mut ops, &mut built, side.as_ref() as *const Tree, &mut next_value);
                            }
                        }
                    }
                    Tree::Mul { left, right } | Tree::Fadd { left, right } | Tree::Fsub { left, right } => {
                        for side in [left, right] {
                            if let Tree::Const(bits) = side.as_ref() {
                                push_const(*bits, &mut nodes, &mut ops, &mut built, side.as_ref() as *const Tree, &mut next_value);
                            }
                        }
                    }
                    _ => {}
                }
            }
            let tree_literals = {
                let mut literals: Vec<u64> = Vec::new();
                fn has_const(tree: &Tree, found: &mut Vec<u64>) {
                    match tree {
                        Tree::Const(bits) => found.push(*bits),
                        Tree::Madd { factor_left, factor_right, addend }
                        | Tree::Fnmsub { factor_left, factor_right, base: addend }
                        | Tree::Fmsub { factor_left, factor_right, subtrahend: addend } => {
                            has_const(factor_left, found);
                            has_const(factor_right, found);
                            has_const(addend, found);
                        }
                        Tree::Mul { left, right } | Tree::Fadd { left, right } | Tree::Fsub { left, right } => {
                            has_const(left, found);
                            has_const(right, found);
                        }
                        _ => {}
                    }
                }
                has_const(local_tree, &mut literals);
                literals
            };
            // A const-bearing shared chain claims ONLY at the measured
            // depths: standalone exactly 3 ariths, in-frame 3..=4 (probed:
            // depth 1-2 DIFF both ways; standalone depth 4 DIFF; in-frame
            // depth 5 DIFF — the deeper schedules interleave the chain to
            // cap the live window, which the frozen order model does not
            // reproduce yet).
            let max_chain = if in_frame { 6 } else { 3 };
            if !tree_literals.is_empty() && (refs.len() < 3 || refs.len() > max_chain) {
                return Ok(false);
            }
            let is_product = refs.len() == 1
                && matches!(local_tree, Tree::Mul { .. } | Tree::Fadd { .. })
                && tree_literals.is_empty();
            local_is_product.push(is_product);
            for (order_index, &(arith, _)) in refs.iter().enumerate() {
                let resolve = |subtree: &Tree, built: &[(*const Tree, Operand)]| -> Option<Operand> {
                    match subtree {
                        Tree::Param(9) if in_frame => Some(Operand::Node(0)),
                        Tree::Param(value) => Some(Operand::Param(*value)),
                        Tree::LocalRef(local) => local_operands.get(*local).copied(),
                        _ => built
                            .iter()
                            .rev()
                            .find(|(key, _)| std::ptr::eq(*key, subtree as *const Tree))
                            .map(|&(_, operand)| operand),
                    }
                };
                let value_of = |operand: Operand, nodes: &Vec<DagNode>| -> u32 {
                    match operand {
                        Operand::Param(value) => value,
                        Operand::Node(index) => nodes[index].writes[0],
                    }
                };
                let index = nodes.len();
                let is_root = order_index + 1 == refs.len();
                let (op, operands): (FloatOp, Vec<Operand>) = match arith {
                    Tree::Madd { factor_left, factor_right, addend } => {
                        let left = resolve(factor_left, &built).ok_or_else(|| Diagnostic::error("dual shared operand"))?;
                        let right = resolve(factor_right, &built).ok_or_else(|| Diagnostic::error("dual shared operand"))?;
                        let b = resolve(addend, &built).ok_or_else(|| Diagnostic::error("dual shared operand"))?;
                        let (a, c) = if matches!(factor_right.as_ref(), Tree::Const(_)) { (right, left) } else { (left, right) };
                        (FloatOp::Madd { a, c, b }, vec![a, c, b])
                    }
                    Tree::Fnmsub { factor_left, factor_right, base } => {
                        let left = resolve(factor_left, &built).ok_or_else(|| Diagnostic::error("dual shared operand"))?;
                        let right = resolve(factor_right, &built).ok_or_else(|| Diagnostic::error("dual shared operand"))?;
                        let b = resolve(base, &built).ok_or_else(|| Diagnostic::error("dual shared operand"))?;
                        let (a, c) = if matches!(factor_right.as_ref(), Tree::Const(_)) { (right, left) } else { (left, right) };
                        (FloatOp::Fnmsub { a, c, b }, vec![a, c, b])
                    }
                    Tree::Fmsub { factor_left, factor_right, subtrahend } => {
                        let left = resolve(factor_left, &built).ok_or_else(|| Diagnostic::error("dual shared operand"))?;
                        let right = resolve(factor_right, &built).ok_or_else(|| Diagnostic::error("dual shared operand"))?;
                        let b = resolve(subtrahend, &built).ok_or_else(|| Diagnostic::error("dual shared operand"))?;
                        let (a, c) = if matches!(factor_right.as_ref(), Tree::Const(_)) { (right, left) } else { (left, right) };
                        (FloatOp::Fmsub { a, c, b }, vec![a, c, b])
                    }
                    Tree::Mul { left, right } => {
                        let a = resolve(left, &built).ok_or_else(|| Diagnostic::error("dual shared operand"))?;
                        let c = resolve(right, &built).ok_or_else(|| Diagnostic::error("dual shared operand"))?;
                        (FloatOp::Mul { a, c }, vec![a, c])
                    }
                    Tree::Fadd { left, right } => {
                        let a = resolve(left, &built).ok_or_else(|| Diagnostic::error("dual shared operand"))?;
                        let b = resolve(right, &built).ok_or_else(|| Diagnostic::error("dual shared operand"))?;
                        (FloatOp::Add { a, b }, vec![a, b])
                    }
                    _ => return Ok(false),
                };
                let reads: Vec<u32> = operands.iter().map(|&operand| value_of(operand, &nodes)).collect();
                let mut node = DagNode::new(if is_product { "flocal" } else { "fshared" }, FLOAT_ARITH_LATENCY)
                    .hazard(HAZARD_FPU)
                    .reads(&reads)
                    .writes(&[next_value]);
                next_value += 1;
                if matches!(op, FloatOp::Mul { .. }) {
                    node = node.gate(FLOAT_MUL_GATE);
                }
                if is_product && is_root {
                    node = node.local_home();
                }
                nodes.push(node);
                ops.push(op);
                if is_root {
                    local_operands.push(Operand::Node(index));
                    built.push((arith as *const Tree, Operand::Node(index)));
                } else {
                    built.push((arith as *const Tree, Operand::Node(index)));
                }
            }
        }
        // The SINK: a store reading every escaping local + every tail-read
        // param — it drives liveness/window and emits nothing.
        let mut sink_reads: Vec<u32> = Vec::new();
        for &index in &escaping_locals {
            if let Operand::Node(node) = local_operands[index] {
                sink_reads.push(nodes[node].writes[0]);
            }
        }
        for (name, value) in &param_ids {
            if reads_of(then_value, name) + reads_of(else_value, name) > 0 {
                sink_reads.push(*value);
            }
        }
        if !nodes.is_empty() {
            nodes.push(DagNode::new("sink", 1).kind(OpKind::Store).reads(&sink_reads));
            ops.push(FloatOp::Sink);
        }

        let synthetic = |value: &Expression| Function {
            return_type: function.return_type,
            name: function.name.clone(),
            is_static: function.is_static,
            is_weak: function.is_weak,
            parameters: function.parameters.clone(),
            locals: Vec::new(),
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: Some(value.clone()),
        };
        // ---- shared registers + emission ----
        let instructions_before = self.output.instructions.len();
        let relocations_before = self.output.relocations.len();
        let bump_before = self.output.anonymous_label_bump;
        let rollback = |generator: &mut Generator| {
            generator.output.instructions.truncate(instructions_before);
            generator.output.relocations.truncate(relocations_before);
            generator.output.anonymous_label_bump = bump_before;
            generator.float_pseudo_params.clear();
        };
        let mut condition_encoding: Option<(u8, u8)> = None;
        if !nodes.is_empty() {
            let order = linearize(&nodes);
            let mut model = FROZEN_FLOAT_REG;
            model.tier_position_desc = true;
            model.window_floor = window_floor;
            // The sink absorbs every consumer, so there is no return node;
            // the shared DAG still allocates on the reverse/tier machine.
            model.void_forward = false;
            let registers = assign_float_registers(&nodes, &order, &params, model);
            if registers.iter().enumerate().any(|(index, register)| {
                register.is_none() && !matches!(ops[index], FloatOp::Sink)
            }) {
                return Ok(false);
            }
            let mut registers = registers;
            // Pool constants intern in SOURCE order (the frozen convention):
            // the locals' initializers left-to-right BEFORE the tail
            // dry-runs below intern the tails' literals — while the lfd's
            // emit in schedule order (measured: ksin_dual's .sdata2, and
            // the 1049 canary's pool caught the dry-run reordering).
            for local in &function.locals {
                if let Some(init) = local.initializer.as_ref() {
                    let mut literals: Vec<u64> = Vec::new();
                    collect_literals(init, &mut literals);
                    for bits in literals {
                        self.output.intern_constant(bits, 8);
                    }
                }
            }
            // THE ESCAPING-ROOT RULE (fire 365, causal probes): the non-tier
            // chain root allocates ASCENDING, skipping registers each tail
            // claims BEFORE the root's last read on that path, plus
            // everything still live at the root's definition. Both tails
            // dry-run with the root as a HIGH placeholder pseudo (f30 — the
            // tails' scratch fills below it, independent), the claims are
            // harvested from the emitted instructions, and the register is
            // fixed before the shared emission (deterministic re-claim).
            let chain_locals: Vec<usize> = (0..local_trees.len())
                .filter(|&index| !local_is_product[index] && escaping_locals.contains(&index))
                .collect();
            let composition = self.float_else_composition.clone();
            let else_synthetic = || -> Function {
                let payload = composition.as_ref().expect("checked at use");
                Function {
                    return_type: function.return_type,
                    name: function.name.clone(),
                    is_static: function.is_static,
                    is_weak: function.is_weak,
                    parameters: function.parameters.clone(),
                    locals: payload.else_locals.clone(),
                    statements: Vec::new(),
                    guards: Vec::new(),
                    return_expression: Some(else_value.clone()),
                }
            };
            if let [chain_index] = chain_locals.as_slice() {
                let Operand::Node(root_node) = local_operands[*chain_index] else {
                    return Ok(false);
                };
                let mut dry_pseudos: Vec<(String, u8)> = Vec::new();
                for &index in &escaping_locals {
                    if let Operand::Node(node) = local_operands[index] {
                        let register = if index == *chain_index {
                            30
                        } else {
                            registers[node].expect("checked above")
                        };
                        dry_pseudos.push((local_names[index].0.clone(), register));
                    }
                }
                let mut x_pseudo_name: Option<String> = None;
                if in_frame {
                    if let Some((name, _)) = param_ids.iter().find(|(_, value)| *value == 9) {
                        x_pseudo_name = Some(name.clone());
                        dry_pseudos.push((name.clone(), registers[0].expect("checked above")));
                    }
                }
                let saved_pseudo = std::mem::take(&mut self.float_pseudo_params);
                let saved_reload = self.float_reload_x.take();
                let mut exclusions: Vec<u8> = Vec::new();
                for (tail, is_else) in [(then_value, false), (else_value, true)] {
                    // The COMPOSED else tail re-reads x and the diamond local
                    // from the FRAME (no x pseudo) and owns the fold locals.
                    let composed = is_else && composition.is_some();
                    self.float_pseudo_params = dry_pseudos.clone();
                    if composed {
                        if let Some(name) = &x_pseudo_name {
                            self.float_pseudo_params.retain(|(pseudo, _)| pseudo != name);
                        }
                        let payload = composition.as_ref().expect("checked");
                        self.float_reload_x = Some(8);
                        self.float_frame_local = Some((payload.qx_name.clone(), payload.qx_offset));
                    }
                    let mark = self.output.instructions.len();
                    let relocation_mark = self.output.relocations.len();
                    let bump_mark = self.output.anonymous_label_bump;
                    let claimed = if composed {
                        self.try_float_dag_return(&else_synthetic())
                    } else {
                        self.try_float_dag_return(&synthetic(tail))
                    };
                    if composed {
                        self.float_reload_x = None;
                        self.float_frame_local = None;
                    }
                    let claimed_ok = matches!(claimed, Ok(true));
                    let mut last_read: Option<usize> = None;
                    for (offset, instruction) in self.output.instructions[mark..].iter().enumerate() {
                        if float_reads_register(instruction, 30) {
                            last_read = Some(offset);
                        }
                    }
                    if let Some(last) = last_read {
                        for instruction in &self.output.instructions[mark..mark + last] {
                            if let Some(register) = float_def(instruction) {
                                if !exclusions.contains(&register) {
                                    exclusions.push(register);
                                }
                            }
                        }
                    }
                    self.output.instructions.truncate(mark);
                    self.output.relocations.truncate(relocation_mark);
                    self.output.anonymous_label_bump = bump_mark;
                    if !claimed_ok || last_read.is_none() {
                        self.float_pseudo_params = saved_pseudo;
                        self.float_reload_x = saved_reload;
                        return claimed.map(|_| false);
                    }
                }
                self.float_pseudo_params = saved_pseudo;
                self.float_reload_x = saved_reload;
                // Everything still live at (or beyond) the root's slot.
                let root_position = order.iter().position(|&node| node == root_node).expect("scheduled");
                let live_end = |node: usize| -> usize {
                    let write = nodes[node].writes.first().copied();
                    (0..nodes.len())
                        .filter(|&reader| {
                            write.is_some_and(|value| nodes[reader].reads.contains(&value))
                        })
                        .map(|reader| order.iter().position(|&slot| slot == reader).unwrap_or(0))
                        .max()
                        .unwrap_or(root_position)
                };
                for node in 0..nodes.len() {
                    if node == root_node {
                        continue;
                    }
                    if let Some(register) = registers[node] {
                        // Dying exactly AT the root is reusable — only
                        // values living BEYOND it exclude their register.
                        if live_end(node) > root_position && !exclusions.contains(&register) {
                            exclusions.push(register);
                        }
                    }
                }
                for &(_, register) in &params {
                    if !exclusions.contains(&register) {
                        exclusions.push(register);
                    }
                }
                let Some(root_register) = (0..32u8).find(|register| !exclusions.contains(register)) else {
                    return Ok(false);
                };
                registers[root_node] = Some(root_register);
            }
            let register_of = |operand: Operand| -> u8 {
                match operand {
                    Operand::Param(value) => params.iter().find(|&&(v, _)| v == value).map(|&(_, register)| register).unwrap_or(1),
                    Operand::Node(index) => registers[index].unwrap_or(1),
                }
            };
            let shared_loads = ops.iter().filter(|op| matches!(op, FloatOp::Const(_))).count();
            let mut emitted_ops = 0usize;
            let mut emitted_loads = 0usize;
            for &node in &order {
                if matches!(ops[node], FloatOp::Sink) {
                    continue;
                }
                let d = registers[node].expect("checked above");
                match &ops[node] {
                    FloatOp::Const(bits) => {
                        self.load_double_constant(d, *bits);
                        emitted_loads += 1;
                    }
                    FloatOp::FrameLoad(offset) => {
                        self.output.instructions.push(Instruction::LoadFloatDouble { d, a: 1, offset: *offset });
                        emitted_loads += 1;
                        if let Some((high, low, _)) = self.float_dual_compare {
                            // The big compare constant materializes right
                            // after the reload: lis r3 + addi r0 (measured).
                            self.output.instructions.push(Instruction::load_immediate_shifted(3, high));
                            self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: low });
                        }
                    }
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
                        let (mut ra, mut rc) = (register_of(*a), register_of(*c));
                        let both_params = matches!(a, Operand::Param(_)) && matches!(c, Operand::Param(_));
                        let const_a = matches!(a, Operand::Node(index) if matches!(ops[*index], FloatOp::Const(_)));
                        if !both_params && !const_a && rc > ra {
                            std::mem::swap(&mut ra, &mut rc);
                        }
                        self.output.instructions.push(Instruction::FloatMultiplyDouble { d, a: ra, c: rc });
                    }
                    FloatOp::Add { a, b } => self.output.instructions.push(Instruction::FloatAddDouble {
                        d,
                        a: register_of(*a),
                        b: register_of(*b),
                    }),
                    _ => {
                        rollback(self);
                        return Ok(false);
                    }
                }
                emitted_ops += 1;
                // The compare interleaves after the SECOND shared load
                // (measured: A/ksin_dual), or after the FIRST float op
                // when the shared DAG is loadless (measured: fire-350).
                // IN-FRAME the compare lands right after the x reload
                // (measured: the real k_sin, cmpwi at slot 1) — except the
                // BIG-const form, whose cmpw lands after the FOURTH shared
                // load (measured at chain depths 3 and 4).
                if let Some((_, _, ix_register)) = self.float_dual_compare {
                    if condition_encoding.is_none() && emitted_loads == 4 {
                        self.output.instructions.push(Instruction::CompareWord { a: ix_register, b: 0 });
                        // Less against the materialized constant: skip to
                        // the else tail on bge.
                        condition_encoding = Some((4, 0));
                    }
                } else {
                    let trigger = if in_frame {
                        emitted_loads == 1 && matches!(ops[node], FloatOp::FrameLoad(_))
                    } else if shared_loads >= 2 {
                        emitted_loads == 2 && matches!(ops[node], FloatOp::Const(_))
                    } else {
                        emitted_ops == 1
                    };
                    if condition_encoding.is_none() && trigger {
                        condition_encoding = Some(self.emit_condition_test(&guard.condition)?);
                    }
                }
            }
            // Pseudo-params for the tails.
            for &index in &escaping_locals {
                if let Operand::Node(node) = local_operands[index] {
                    self.float_pseudo_params.push((local_names[index].0.clone(), registers[node].expect("checked above")));
                }
            }
            if in_frame {
                if let Some((name, _)) = param_ids.iter().find(|(_, value)| *value == 9) {
                    self.float_pseudo_params.push((name.clone(), registers[0].expect("checked above")));
                }
            }
        }
        // The tails see x through its pseudo-param, not a re-reload.
        self.float_reload_x = None;
        let (options, condition_bit) = match condition_encoding {
            Some(encoding) => encoding,
            None => self.emit_condition_test(&guard.condition)?,
        };
        let branch_index = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
        // The if pair + the else-join label (measured: pools at @8/@9).
        self.output.anonymous_label_bump += 3;
        match self.try_float_dag_return(&synthetic(then_value)) {
            Ok(true) => {}
            other => {
                rollback(self);
                return other.map(|_| false);
            }
        }
        let mut then_join: Option<usize> = None;
        if in_frame {
            // The then tail joins the caller's shared epilogue (measured:
            // b <addi;blr>); the else tail falls through into it.
            then_join = Some(self.output.instructions.len());
            self.output.instructions.push(Instruction::Branch { target: 0 });
        } else {
            self.output.instructions.push(Instruction::BranchToLinkRegister);
        }
        let else_start = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
            *target = else_start;
        }
        let composition_real = self.float_else_composition.clone();
        if let Some(payload) = &composition_real {
            // The inner diamond opens the else branch: lis r0 + cmpw against
            // the preserved ix, ble to the punned arm, the literal arm
            // through the f0 scratch, the addis/li/stw/stw arm, join.
            self.output.instructions.push(Instruction::load_immediate_shifted(0, payload.compare_high));
            self.output.instructions.push(Instruction::CompareWord { a: payload.ix_register, b: 0 });
            let skip_index = self.output.instructions.len();
            self.output.instructions.push(Instruction::BranchConditionalForward {
                options: payload.skip_options,
                condition_bit: payload.skip_bit,
                target: 0,
            });
            self.load_double_constant(0, payload.then_bits);
            self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: payload.qx_offset });
            let join_index = self.output.instructions.len();
            self.output.instructions.push(Instruction::Branch { target: 0 });
            let diamond_else = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[skip_index] {
                *target = diamond_else;
            }
            self.output.instructions.push(Instruction::AddImmediateShifted {
                d: payload.addis_target,
                a: payload.ix_register,
                immediate: payload.addis_shift,
            });
            self.output.instructions.push(Instruction::load_immediate(0, 0));
            self.output.instructions.push(Instruction::StoreWord { s: payload.addis_target, a: 1, offset: payload.qx_offset });
            self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: payload.qx_offset + 4 });
            let join = self.output.instructions.len();
            if let Instruction::Branch { target } = &mut self.output.instructions[join_index] {
                *target = join;
            }
            self.output.anonymous_label_bump += 3;
            // The composed tail: x re-reloads, the diamond local frame-reads.
            if let Some((name, _)) = param_ids.iter().find(|(_, value)| *value == 9) {
                self.float_pseudo_params.retain(|(pseudo, _)| pseudo != name);
            }
            self.float_reload_x = Some(8);
            self.float_frame_local = Some((payload.qx_name.clone(), payload.qx_offset));
            let composed_synthetic = Function {
                return_type: function.return_type,
                name: function.name.clone(),
                is_static: function.is_static,
                is_weak: function.is_weak,
                parameters: function.parameters.clone(),
                locals: payload.else_locals.clone(),
                statements: Vec::new(),
                guards: Vec::new(),
                return_expression: Some(else_value.clone()),
            };
            let claimed = self.try_float_dag_return(&composed_synthetic);
            self.float_reload_x = None;
            self.float_frame_local = None;
            match claimed {
                Ok(true) => {}
                other => {
                    rollback(self);
                    return other.map(|_| false);
                }
            }
        } else {
            match self.try_float_dag_return(&synthetic(else_value)) {
                Ok(true) => {}
                other => {
                    rollback(self);
                    return other.map(|_| false);
                }
            }
        }
        if in_frame {
            let epilogue = self.output.instructions.len();
            if let Some(index) = then_join {
                if let Instruction::Branch { target } = &mut self.output.instructions[index] {
                    *target = epilogue;
                }
            }
        } else {
            self.output.instructions.push(Instruction::BranchToLinkRegister);
        }
        self.float_pseudo_params.clear();
        Ok(true)
    }

    /// The CONDITIONAL-LOCAL diamond (k_cos's qx, register form): a leading
    /// `if (c) { qx = A; } else { qx = B; }` over one double local, then a
    /// float return reading qx. Both arms load the SAME register — the one
    /// the tail's DAG assigns qx as a window-top tier value (the PHANTOM
    /// node) — and fall through the join into the tail (measured: the
    /// four-tail register battery; `return qx` alone stays deferred).
    pub(crate) fn try_conditional_local_float_return(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        if function.return_type != Type::Double
            || !function.guards.is_empty()
            || function.statements.len() != 1
            || function.locals.len() != 1
        {
            return Ok(false);
        }
        let Some(return_expression) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        let local = &function.locals[0];
        if local.declared_type != Type::Double || local.initializer.is_some() || local.array_length.is_some() {
            return Ok(false);
        }
        let qx = local.name.as_str();
        if matches!(return_expression, Expression::Variable(_)) {
            return Ok(false);
        }
        let Some(Statement::If { condition, then_body, else_body }) = function.statements.first() else {
            return Ok(false);
        };
        let arm_literal = |body: &[Statement]| -> Option<u64> {
            match body {
                [Statement::Assign { name, value: Expression::FloatLiteral(value) }] if name == qx => {
                    Some(value.to_bits())
                }
                _ => None,
            }
        };
        // The FRAME-punned else arm (k_cos's qx): `*(int*)&qx = HI;
        // *((int*)&qx+1) = 0;` — HI a general leaf (stw direct) or
        // leaf±lis-able-constant (addis into the freed condition register).
        let punned_else: Option<&Expression> = match else_body.as_slice() {
            [Statement::Store { target: hi_target, value: hi_value }, Statement::Store { target: lo_target, value: lo_value }]
                if crate::frame::pun_word_offset_pub(hi_target, qx) == Some(0)
                    && crate::frame::pun_word_offset_pub(lo_target, qx) == Some(4)
                    && crate::analysis::constant_value(lo_value) == Some(0) =>
            {
                Some(hi_value)
            }
            _ => None,
        };
        let Some(then_bits) = arm_literal(then_body) else {
            return Ok(false);
        };
        let else_bits = arm_literal(else_body);
        if punned_else.is_none() {
            let Some(else_bits) = else_bits else {
                return Ok(false);
            };
            if then_bits == else_bits {
                return Ok(false);
            }
        }
        // The punned form's HI store: (leaf register, optional addis shift).
        let general_leaf = |expression: &Expression| -> Option<u8> {
            match expression {
                Expression::Variable(name) => self
                    .locations
                    .get(name)
                    .filter(|location| location.class == ValueClass::General)
                    .map(|location| location.register),
                _ => None,
            }
        };
        let punned_hi: Option<(u8, Option<i16>)> = match punned_else {
            None => None,
            Some(Expression::Variable(_)) => {
                let Some(register) = general_leaf(punned_else.expect("checked")) else {
                    return Ok(false);
                };
                Some((register, None))
            }
            Some(Expression::Binary { operator, left, right })
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract) =>
            {
                // addis needs a plain-Variable condition whose register the
                // shifted add reuses (measured: addis r3,r4,-32).
                if !matches!(condition, Expression::Variable(_)) {
                    return Ok(false);
                }
                let Some(register) = general_leaf(left) else {
                    return Ok(false);
                };
                let Some(constant) = crate::analysis::constant_value(right) else {
                    return Ok(false);
                };
                let signed = if matches!(operator, BinaryOperator::Subtract) { -constant } else { constant };
                if signed & 0xffff != 0 {
                    return Ok(false);
                }
                Some((register, Some((signed >> 16) as i16)))
            }
            Some(_) => return Ok(false),
        };
        if punned_else.is_some() && punned_hi.is_none() {
            return Ok(false);
        }
        // The tail: qx free, no locals (a diamond alongside other locals is
        // unmeasured).
        let synthetic = Function {
            return_type: function.return_type,
            name: function.name.clone(),
            is_static: function.is_static,
            is_weak: function.is_weak,
            parameters: function.parameters.clone(),
            locals: Vec::new(),
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: Some(return_expression.clone()),
        };
        // Pool constants intern in SOURCE order: the arms' literals precede
        // the tail's.
        self.output.intern_constant(then_bits, 8);
        if let Some(bits) = else_bits {
            self.output.intern_constant(bits, 8);
        }
        let instructions_before = self.output.instructions.len();
        let relocations_before = self.output.relocations.len();
        let bump_before = self.output.anonymous_label_bump;
        let frame_before = self.frame_size;
        let rollback = |generator: &mut Generator| {
            generator.output.instructions.truncate(instructions_before);
            generator.output.relocations.truncate(relocations_before);
            generator.output.anonymous_label_bump = bump_before;
            generator.frame_size = frame_before;
        };
        if let Some((hi_register, addis_shift)) = punned_hi {
            // D2, the FRAME form: cmpwi HOISTS above the stwu; the then arm
            // stores through the f0 scratch; the else arm addis?/li/stw/stw;
            // the tail reads qx as a FrameLoad (measured: the D2 battery).
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            self.frame_size = 16;
            self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
            let skip_index = self.output.instructions.len();
            self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
            self.load_double_constant(0, then_bits);
            self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 8 });
            let join_index = self.output.instructions.len();
            self.output.instructions.push(Instruction::Branch { target: 0 });
            let else_start = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[skip_index] {
                *target = else_start;
            }
            let hi_store_register = match addis_shift {
                Some(shift) => {
                    let Expression::Variable(name) = condition else {
                        unreachable!("gated above")
                    };
                    let condition_register = self
                        .locations
                        .get(name)
                        .map(|location| location.register)
                        .expect("condition tested above");
                    self.output.instructions.push(Instruction::AddImmediateShifted {
                        d: condition_register,
                        a: hi_register,
                        immediate: shift,
                    });
                    condition_register
                }
                None => hi_register,
            };
            self.output.instructions.push(Instruction::load_immediate(0, 0));
            self.output.instructions.push(Instruction::StoreWord { s: hi_store_register, a: 1, offset: 8 });
            self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 12 });
            let join = self.output.instructions.len();
            if let Instruction::Branch { target } = &mut self.output.instructions[join_index] {
                *target = join;
            }
            self.float_frame_local = Some((qx.to_string(), 8));
            let claimed = self.try_float_dag_return(&synthetic);
            self.float_frame_local = None;
            match claimed {
                Ok(true) => {}
                other => {
                    rollback(self);
                    return other.map(|_| false);
                }
            }
            self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            // The FRAME form consumes 3 labels like the register form; the
            // root DECONTRACTION adds its own +1 inside the claim (measured:
            // pool @9 = bump 4 decontracted, extab @41 = bump 3 contracted).
            self.output.anonymous_label_bump += 3;
            return Ok(true);
        }
        let else_bits = else_bits.expect("checked above");
        // PASS 1 (dry): learn qx's register from the tail's allocation.
        self.float_phantom_local = Some(qx.to_string());
        self.float_phantom_register = None;
        let dry = self.try_float_dag_return(&synthetic);
        let register = self.float_phantom_register;
        self.output.instructions.truncate(instructions_before);
        self.output.relocations.truncate(relocations_before);
        self.output.anonymous_label_bump = bump_before;
        let (Ok(true), Some(register)) = (&dry, register) else {
            self.float_phantom_local = None;
            self.float_phantom_register = None;
            return dry.map(|_| false);
        };
        // The diamond: test; skip to the else arm; then-load; join branch.
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let skip_index = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
        self.load_double_constant(register, then_bits);
        let join_index = self.output.instructions.len();
        self.output.instructions.push(Instruction::Branch { target: 0 });
        let else_start = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[skip_index] {
            *target = else_start;
        }
        self.load_double_constant(register, else_bits);
        let join = self.output.instructions.len();
        if let Instruction::Branch { target } = &mut self.output.instructions[join_index] {
            *target = join;
        }
        // PASS 2 (real): the claim is deterministic — same register.
        let claimed = self.try_float_dag_return(&synthetic);
        self.float_phantom_local = None;
        self.float_phantom_register = None;
        match claimed {
            Ok(true) => {}
            other => {
                rollback(self);
                return other.map(|_| false);
            }
        }
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // The if pair + the join label (measured via objprobe).
        self.output.anonymous_label_bump += 3;
        Ok(true)
    }
}
