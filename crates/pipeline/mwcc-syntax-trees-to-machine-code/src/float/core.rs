//! The CORE float DAG return arm (try_float_dag_return): the frozen
//! order + register models over one return tree.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, Type};
use mwcc_vreg::{assign_float_registers, linearize, DagNode, FROZEN_FLOAT_REG, HAZARD_FPU};
use crate::generator::*;
use super::{
    build_tree, collect_arith, collect_literals, count_arith, FloatOp, Operand, Tree, FLOAT_ARITH_LATENCY, FLOAT_MUL_GATE, LOAD_LATENCY,
};

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
        let reload_mode = self.float.reload_x.is_some();
        for (name, register) in &self.float.pseudo_params {
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
        if let Some(name) = &self.float.phantom_local {
            // The diamond-defined local reads as value id 8 (the phantom
            // node's write), the same convention as the reload's 9.
            param_ids.push((name.clone(), 8));
        }
        if let Some((name, _)) = &self.float.frame_local {
            // The frame-resident diamond local reads as value id 7 — a
            // FrameLoad node in the tail's DAG.
            param_ids.push((name.clone(), 7));
        }
        // Lower the expression to the contracted tree (or bail). ONE
        // coefficient table may feed the claim (constant indices only).
        let mut table_context = super::TableContext {
            name: None,
            tables: self.double_tables.clone(),
        };
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
            let Some(local_tree) = super::build_tree_with_tables(initializer, &param_ids, prior, &mut seen_literals, &mut table_context) else {
                return Ok(false);
            };
            if count_arith(&local_tree) != 1 || !seen_literals.is_empty() {
                return Ok(false);
            }
            local_trees.push(local_tree);
        }
        let Some(tree) = super::build_tree_with_tables(return_expression, &param_ids, &local_names, &mut seen_literals, &mut table_context) else {
            return Ok(false);
        };
        if table_context.name.is_some() {
            // An arith mixing a POOL constant and a TABLE constant ties
            // their deaths — the register tiebreak is unfitted (probed:
            // 0.5->f0/T->f2 against descending) — and multi-local table
            // shapes (the s_atan z/w split) are uncaptured. Defer both.
            let mut refs: Vec<(&Tree, u32)> = Vec::new();
            collect_arith(&tree, 0, &mut refs);
            for (arith, _) in &refs {
                let sides: Vec<&Tree> = match arith {
                    Tree::Madd { factor_left, factor_right, addend }
                    | Tree::Fnmsub { factor_left, factor_right, base: addend }
                    | Tree::Fmsub { factor_left, factor_right, subtrahend: addend } => {
                        vec![factor_left, factor_right, addend]
                    }
                    Tree::Mul { left, right } | Tree::Fadd { left, right } | Tree::Fsub { left, right } => {
                        vec![left, right]
                    }
                    _ => Vec::new(),
                };
                let pools = sides.iter().filter(|side| matches!(side, Tree::Const(_))).count();
                let tables = sides.iter().filter(|side| matches!(side, Tree::TableConst(_))).count();
                if pools > 0 && tables > 0 {
                    return Ok(false);
                }
            }
            // Multi-local table shapes: the SHALLOW class (z,w with at most
            // three ariths in the folded return — one chain link per parity)
            // is fitted; the deeper interleave (s_atan's full split) is not.
            if kept_locals.len() > 2 || (kept_locals.len() == 2 && count_arith(&tree) > 3) {
                return Ok(false);
            }
        }
        if table_context.name.is_some()
            && (!function.guards.is_empty()
                || self.float.reload_x.is_some()
                || self.float.frame_local.is_some()
                || self.float.phantom_local.is_some()
                || !self.float.pseudo_params.is_empty())
        {
            // Tables are measured in the plain return claim only — the
            // composed/guarded environments defer until probed.
            return Ok(false);
        }
        if table_context.name.is_some() {
            // The base takes r3 — measured only with every int param DEAD
            // in the claimed body.
            for parameter in &function.parameters {
                if parameter.parameter_type == Type::Double {
                    continue;
                }
                let reads = crate::analysis::count_name_occurrences(return_expression, &parameter.name)
                    + function
                        .locals
                        .iter()
                        .filter_map(|local| local.initializer.as_ref())
                        .map(|init| crate::analysis::count_name_occurrences(init, &parameter.name))
                        .sum::<usize>();
                if reads != 0 {
                    return Ok(false);
                }
            }
        }
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
        let tree = if self.float.frame_local.is_some() {
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
                && self.float.phantom_local.is_none()
                && self.float.pseudo_params.is_empty()
                && table_context.name.is_none())
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
        if let Some(offset) = self.float.reload_x {
            let index = nodes.len();
            nodes.push(DagNode::new("lfd_x", LOAD_LATENCY).writes(&[9]));
            ops.push(FloatOp::FrameLoad(offset));
            reload_operand = Some(Operand::Node(index));
            let _ = reload_operand;
        }
        let mut phantom_operand: Option<Operand> = None;
        let mut phantom_index: Option<usize> = None;
        if self.float.phantom_local.is_some() {
            let index = nodes.len();
            nodes.push(DagNode::new("phantom", 0).local_home().writes(&[8]));
            ops.push(FloatOp::Phantom);
            phantom_operand = Some(Operand::Node(index));
            phantom_index = Some(index);
        }
        let mut frame_local_operand: Option<Operand> = None;
        if let Some((_, offset)) = self.float.frame_local {
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
            let push_table = |offset: i16, nodes: &mut Vec<DagNode>, ops: &mut Vec<FloatOp>, built: &mut Vec<(*const Tree, Operand)>, key: *const Tree| {
                let index = nodes.len();
                nodes.push(DagNode::new("lfd_t", LOAD_LATENCY).writes(&[10 + index as u32]));
                ops.push(FloatOp::TableLoad(offset));
                built.push((key, Operand::Node(index)));
            };
            let push_const = |bits: u64, nodes: &mut Vec<DagNode>, ops: &mut Vec<FloatOp>, built: &mut Vec<(*const Tree, Operand)>, key: *const Tree| {
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
                    // Table reads load AFTER the arith's pool constants
                    // (measured: 0.5 then T[2] in the mixed probe).
                    for factor in [factor_left, factor_right] {
                        if let Tree::TableConst(offset) = factor.as_ref() {
                            push_table(*offset, &mut nodes, &mut ops, &mut built, factor.as_ref() as *const Tree);
                        }
                    }
                    if let Tree::TableConst(offset) = addend.as_ref() {
                        push_table(*offset, &mut nodes, &mut ops, &mut built, addend.as_ref() as *const Tree);
                    }
                }
                Tree::Mul { left, right } | Tree::Fadd { left, right } | Tree::Fsub { left, right } => {
                    for side in [left, right] {
                        if let Tree::Const(bits) = side.as_ref() {
                            push_const(*bits, &mut nodes, &mut ops, &mut built, side.as_ref() as *const Tree);
                        }
                    }
                    for side in [left, right] {
                        if let Tree::TableConst(offset) = side.as_ref() {
                            push_table(*offset, &mut nodes, &mut ops, &mut built, side.as_ref() as *const Tree);
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
        // The COMPOSED tail (x re-reload + a frame local together) runs the
        // emission sequence over the whole DAG (measured: the k_cos else).
        let model = {
            let mut model = FROZEN_FLOAT_REG;
            model.emission_over_tier =
                self.float.reload_x.is_some() && self.float.frame_local.is_some();
            model
        };
        let registers = assign_float_registers(&nodes, &order, &params, model);
        if let Some(index) = phantom_index {
            self.float.phantom_register = registers[index];
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
        // The TABLE BASE weave (measured: lis at slot 0 before the first
        // float op, addi at slot 2): lis+@ha up front; the low half lands
        // right after the first emitted float instruction.
        // TWO kept locals (z, w): the base PAIR — lis and addi together —
        // lands after the first float instruction (measured: the shallow
        // and deep table shapes all open fmul z; lis; addi). A single
        // local splits them around the first float op.
        let table_pair_after_first = table_context.name.is_some() && kept_locals.len() >= 2;
        let mut table_addi_pending = false;
        if let Some(name) = &table_context.name {
            if !table_pair_after_first {
                self.emit_address_high(3, name);
                table_addi_pending = true;
                // Nothing schedulable before the first table read: the low
                // half follows the lis directly (measured: the single-op
                // claim).
                if order
                    .first()
                    .is_some_and(|&node| matches!(ops[node], FloatOp::TableLoad(_)))
                {
                    self.record_relocation(RelocationKind::Addr16Lo, name);
                    self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
                    table_addi_pending = false;
                }
            }
        }
        let table_name = table_context.name.clone();
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
                FloatOp::TableLoad(offset) => {
                    self.output.instructions.push(Instruction::LoadFloatDouble { d, a: 3, offset: *offset })
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
                    // A FRAME LOAD (the x re-reload, the diamond local) is
                    // param-LIKE for slot order: source order holds, no
                    // register-DESC swap (measured: k_cos's fmul f0,f0,f2).
                    let param_like = |operand: &Operand| {
                        matches!(operand, Operand::Param(_))
                            || matches!(operand, Operand::Node(index) if matches!(ops[*index], FloatOp::FrameLoad(_)))
                    };
                    let both_params = param_like(a) && param_like(c);
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
            if table_addi_pending && emitted == 1 {
                // The base's low half: addi rB,rB,T@l right after the first
                // float instruction (measured: slot 2 in every table probe).
                self.record_relocation(
                    RelocationKind::Addr16Lo,
                    table_name.as_deref().expect("pending implies a table"),
                );
                self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
                table_addi_pending = false;
            }
            if table_pair_after_first && emitted == 1 {
                let name = table_name.as_deref().expect("pair implies a table");
                self.emit_address_high(3, name);
                self.record_relocation(RelocationKind::Addr16Lo, name);
                self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: 0 });
            }
        }
        let _ = table_addi_pending;
        Ok(true)
    }
}
