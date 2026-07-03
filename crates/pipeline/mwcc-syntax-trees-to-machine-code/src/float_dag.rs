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
        // The locals' shared nodes, in declaration order (their arith emits
        // ahead of the loads: measured, z's fmul is slot 0).
        let mut local_operands: Vec<Operand> = Vec::new();
        for local_tree in &local_trees {
            let resolve_leaf = |leaf: &Tree| -> Option<Operand> {
                match leaf {
                    Tree::Param(9) => reload_operand,
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
        let nested = matches!(
            function.statements.first(),
            Some(Statement::If { .. }) if function.guards.is_empty() || dual_tail
        );
        // Trailing `z = x*x;`-style assigns behind the nested if become
        // initializers (each target a declared, uninitialized local,
        // assigned once).
        let mut trailing_inits: Vec<(String, Expression)> = Vec::new();
        if nested {
            for statement in &function.statements[1..] {
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
            if !inner_else.is_empty()
                || !matches!(inner_then.as_slice(), [Statement::Return(Some(Expression::Variable(name)))] if name == x)
            {
                return Ok(false);
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
        // ix appears nowhere else.
        let ix_uses_elsewhere = function
            .guards
            .iter()
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
            if !trailing_inits.iter().any(|(name, _)| name == &local.name) {
                synthetic_locals.push(local.clone());
            }
        }
        let synthetic = Function {
            return_type: function.return_type,
            name: function.name.clone(),
            is_static: function.is_static,
            is_weak: function.is_weak,
            parameters: function.parameters.clone(),
            locals: synthetic_locals,
            statements: Vec::new(),
            guards: if nested { function.guards.clone() } else { Vec::new() },
            return_expression: function.return_expression.clone(),
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
        let target_register = if lis_high.is_some() { 3 + int_params } else { 0 };
        self.output.instructions.push(Instruction::LoadWord { d: target_register, a: 1, offset: 8 });
        if masked {
            self.output.instructions.push(Instruction::ClearLeftImmediate { a: target_register, s: target_register, clear: 1 });
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
            self.output.instructions.push(Instruction::StoreFloatDouble { s: 0, a: 1, offset: 16 });
            self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
            self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
            tail_branches.push(self.output.instructions.len());
            self.output.instructions.push(Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 0 });
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
        let claimed = if synthetic.guards.is_empty() {
            self.try_float_dag_return(&synthetic)
        } else {
            self.try_dual_tail_float_return(&synthetic)
        };
        self.float_reload_x = None;
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
        if !condition_ok {
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
            local_names.push((local.name.clone(), local_trees.len()));
            local_trees.push(tree);
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
            // A const-bearing shared chain shallower than 3 ariths is an
            // unmeasured placement regime (probed DIFF at depth 1 and 2;
            // k_sin's r at depth 3 matches) — defer.
            if !tree_literals.is_empty() && refs.len() < 3 {
                return Ok(false);
            }
            let is_product = refs.len() == 1
                && matches!(local_tree, Tree::Mul { .. } | Tree::Fadd { .. })
                && tree_literals.is_empty();
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
            let register_of = |operand: Operand| -> u8 {
                match operand {
                    Operand::Param(value) => params.iter().find(|&&(v, _)| v == value).map(|&(_, register)| register).unwrap_or(1),
                    Operand::Node(index) => registers[index].unwrap_or(1),
                }
            };
            // Pool constants intern in SOURCE order (the frozen convention):
            // the locals' initializers left-to-right here; each tail then
            // self-interns its own literals at claim time — while the lfd's
            // emit in schedule order (measured: ksin_dual's .sdata2).
            for local in &function.locals {
                if let Some(init) = local.initializer.as_ref() {
                    let mut literals: Vec<u64> = Vec::new();
                    collect_literals(init, &mut literals);
                    for bits in literals {
                        self.output.intern_constant(bits, 8);
                    }
                }
            }
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
                // (measured: the real k_sin, cmpwi at slot 1).
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
        match self.try_float_dag_return(&synthetic(else_value)) {
            Ok(true) => {}
            other => {
                rollback(self);
                return other.map(|_| false);
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
}
