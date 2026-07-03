//! The DUAL-TAIL arm (try_dual_tail_float_return) and the
//! CONDITIONAL-LOCAL diamond (try_conditional_local_float_return).

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, Type};
use mwcc_vreg::{assign_float_registers, linearize, DagNode, OpKind, FROZEN_FLOAT_REG, HAZARD_FPU};
use crate::generator::*;
use super::{
    build_tree, collect_arith, collect_literals, float_def, float_reads_register, FloatOp, Operand, Tree, FLOAT_ARITH_LATENCY, FLOAT_MUL_GATE, LOAD_LATENCY,
};

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
        if !condition_ok && self.float.dual_compare.is_none() {
            return Ok(false);
        }
        // IN-FRAME (the punned k_sin composition): x lives in the frame —
        // its shared-DAG references become a reload node (value id 9, the
        // core arm's convention) and f1 frees for the chain.
        let in_frame = self.float.reload_x.is_some();
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
            ops.push(FloatOp::FrameLoad(self.float.reload_x.expect("in_frame")));
        }
        for local_tree in &local_trees {
            // Loads first (per chain), then its arith deepest-level first.
            let mut refs: Vec<(&Tree, u32)> = Vec::new();
            collect_arith(local_tree, 0, &mut refs);
            refs.sort_by_key(|&(_, level)| std::cmp::Reverse(level));
            for &(arith, _) in &refs {
                let push_const = |bits: u64, nodes: &mut Vec<DagNode>, ops: &mut Vec<FloatOp>, built: &mut Vec<(*const Tree, Operand)>, key: *const Tree, next_value: &mut u32| {
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
            generator.float.pseudo_params.clear();
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
            let composition = self.float.else_composition.clone();
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
                let saved_pseudo = std::mem::take(&mut self.float.pseudo_params);
                let saved_reload = self.float.reload_x.take();
                let mut exclusions: Vec<u8> = Vec::new();
                for (tail, is_else) in [(then_value, false), (else_value, true)] {
                    // The COMPOSED else tail re-reads x and the diamond local
                    // from the FRAME (no x pseudo) and owns the fold locals.
                    let composed = is_else && composition.is_some();
                    self.float.pseudo_params = dry_pseudos.clone();
                    if composed {
                        if let Some(name) = &x_pseudo_name {
                            self.float.pseudo_params.retain(|(pseudo, _)| pseudo != name);
                        }
                        let payload = composition.as_ref().expect("checked");
                        self.float.reload_x = Some(8);
                        self.float.frame_local = Some((payload.qx_name.clone(), payload.qx_offset));
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
                        self.float.reload_x = None;
                        self.float.frame_local = None;
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
                        self.float.pseudo_params = saved_pseudo;
                        self.float.reload_x = saved_reload;
                        return claimed.map(|_| false);
                    }
                }
                self.float.pseudo_params = saved_pseudo;
                self.float.reload_x = saved_reload;
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
                        if let Some((high, low, _)) = self.float.dual_compare {
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
                if let Some((_, _, ix_register)) = self.float.dual_compare {
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
                    self.float.pseudo_params.push((local_names[index].0.clone(), registers[node].expect("checked above")));
                }
            }
            if in_frame {
                if let Some((name, _)) = param_ids.iter().find(|(_, value)| *value == 9) {
                    self.float.pseudo_params.push((name.clone(), registers[0].expect("checked above")));
                }
            }
        }
        // The tails see x through its pseudo-param, not a re-reload.
        self.float.reload_x = None;
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
        let composition_real = self.float.else_composition.clone();
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
                self.float.pseudo_params.retain(|(pseudo, _)| pseudo != name);
            }
            self.float.reload_x = Some(8);
            self.float.frame_local = Some((payload.qx_name.clone(), payload.qx_offset));
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
            self.float.reload_x = None;
            self.float.frame_local = None;
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
        self.float.pseudo_params.clear();
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
            self.float.frame_local = Some((qx.to_string(), 8));
            let claimed = self.try_float_dag_return(&synthetic);
            self.float.frame_local = None;
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
        self.float.phantom_local = Some(qx.to_string());
        self.float.phantom_register = None;
        let dry = self.try_float_dag_return(&synthetic);
        let register = self.float.phantom_register;
        self.output.instructions.truncate(instructions_before);
        self.output.relocations.truncate(relocations_before);
        self.output.anonymous_label_bump = bump_before;
        let (Ok(true), Some(register)) = (&dry, register) else {
            self.float.phantom_local = None;
            self.float.phantom_register = None;
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
        self.float.phantom_local = None;
        self.float.phantom_register = None;
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

