//! The DAG EMITTER — the keystone campaign's payoff: leaf multi-store bodies
//! compile through the MEASURED models instead of bespoke arms.
//!
//! Statement trees build a dependence DAG ([`mwcc_vreg::DagNode`] + an
//! instruction template per node); [`mwcc_vreg::linearize`] (the frozen v4
//! dual-issue model, 10/10 on the scheduler dataset) orders it; and
//! [`mwcc_vreg::assign_registers_v3`] (10/10 on the register fixtures) chooses
//! every register. This module only recognizes shapes INSIDE the models'
//! validated envelope — int expression trees over parameters, loads through
//! pointer parameters, stores to distinct small-data scalar globals — and
//! defers the rest honestly.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, Pointee, Statement, Type};
use mwcc_versions::GlobalAddressing;
use mwcc_vreg::{assign_registers_v3, linearize, DagNode, OpKind, HAZARD_XER};

use crate::analysis::{constant_value, function_makes_call};
use crate::generator::Generator;

/// The instruction each DAG node emits once its registers are known.
enum Template {
    LoadImmediate(i16),
    AddImmediate(i16),
    Add,
    /// `a - b` with both operands in registers: `subf d,b,a`.
    Subtract,
    MultiplyImmediate(i16),
    ShiftLeftImmediate(u8),
    ShiftRightAlgebraicImmediate(u8),
    OrImmediate(u16),
    OrImmediateShifted(u16),
    /// `rlwinm d,s,0,begin,end` — an AND by a contiguous or wrap mask.
    Mask(u8, u8),
    LoadWord,
    /// A small-data scalar store: `stw s, g@sda21(r0)`.
    StoreGlobal(String),
}

/// A value id source: either a parameter's register or a node's result.
#[derive(Clone, Copy)]
enum ValueSource {
    Parameter(u8),
    Node(usize),
}

struct Builder {
    nodes: Vec<DagNode>,
    templates: Vec<Template>,
    sources: Vec<(u32, ValueSource)>,
    next_value: u32,
}

impl Builder {
    fn value_of(&self, id: u32) -> ValueSource {
        self.sources.iter().find(|(value, _)| *value == id).map(|&(_, source)| source).expect("known value")
    }

    fn push(&mut self, kind: OpKind, latency: u32, gate: u32, reads: Vec<u32>, template: Template) -> u32 {
        let value = self.next_value;
        self.next_value += 1;
        let mut node = DagNode::new("", latency).kind(kind).gate(gate);
        node.reads = reads;
        node.writes = vec![value];
        self.nodes.push(node);
        self.templates.push(template);
        self.sources.push((value, ValueSource::Node(self.nodes.len() - 1)));
        value
    }

    /// Lower an int expression to a DAG value; `None` defers (outside the
    /// validated envelope).
    fn expression(&mut self, expression: &Expression, generator: &Generator) -> Option<u32> {
        // A bare small constant is an `li` node (no reads).
        if let Some(constant) = constant_value(expression) {
            let immediate = i16::try_from(constant).ok()?;
            return Some(self.push(OpKind::Alu, 1, 1, vec![], Template::LoadImmediate(immediate)));
        }
        match expression {
            Expression::Variable(name) => {
                let register = generator.lookup_general(name)?;
                self.sources
                    .iter()
                    .find_map(|&(value, source)| match source {
                        ValueSource::Parameter(parameter) if parameter == register => Some(value),
                        _ => None,
                    })
            }
            // `*p` through a pointer parameter: a word load.
            Expression::Dereference { pointer } => {
                let Expression::Variable(name) = pointer.as_ref() else { return None };
                let location = generator.locations.get(name.as_str())?;
                if location.pointee != Some(Pointee::Int) && location.pointee != Some(Pointee::UnsignedInt) {
                    return None;
                }
                let pointer_value = self.expression(pointer, generator)?;
                Some(self.push(OpKind::Load, 2, 2, vec![pointer_value], Template::LoadWord))
            }
            Expression::Binary { operator, left, right } => {
                let constant_right = constant_value(right);
                let constant_left = constant_value(left);
                match operator {
                    BinaryOperator::Add => {
                        if let Some(constant) = constant_right.or(constant_left) {
                            let operand = if constant_right.is_some() { left } else { right };
                            let immediate = i16::try_from(constant).ok()?;
                            let value = self.expression(operand, generator)?;
                            return Some(self.push(OpKind::Alu, 1, 1, vec![value], Template::AddImmediate(immediate)));
                        }
                        let a = self.expression(left, generator)?;
                        let b = self.expression(right, generator)?;
                        Some(self.push(OpKind::Alu, 1, 1, vec![a, b], Template::Add))
                    }
                    BinaryOperator::Subtract => {
                        if let Some(constant) = constant_right {
                            let immediate = i16::try_from(constant).ok().and_then(|value| value.checked_neg())?;
                            let value = self.expression(left, generator)?;
                            return Some(self.push(OpKind::Alu, 1, 1, vec![value], Template::AddImmediate(immediate)));
                        }
                        let a = self.expression(left, generator)?;
                        let b = self.expression(right, generator)?;
                        Some(self.push(OpKind::Alu, 1, 1, vec![a, b], Template::Subtract))
                    }
                    BinaryOperator::Multiply => {
                        let constant = constant_right.or(constant_left)?;
                        let operand = if constant_right.is_some() { left } else { right };
                        let value = self.expression(operand, generator)?;
                        if constant > 0 && (constant as u64).is_power_of_two() {
                            let shift = (constant as u64).trailing_zeros() as u8;
                            return Some(self.push(OpKind::Alu, 1, 1, vec![value], Template::ShiftLeftImmediate(shift)));
                        }
                        let immediate = i16::try_from(constant).ok()?;
                        // mulli weighs 3 for priority but gates consumers at 2 (measured).
                        Some(self.push(OpKind::Alu, 3, 2, vec![value], Template::MultiplyImmediate(immediate)))
                    }
                    BinaryOperator::ShiftRight => {
                        let shift = u8::try_from(constant_right?).ok().filter(|shift| *shift < 32)?;
                        let value = self.expression(left, generator)?;
                        // srawi writes XER.CA — two cannot dual-issue (measured).
                        let node = self.push(OpKind::Alu, 1, 1, vec![value], Template::ShiftRightAlgebraicImmediate(shift));
                        self.nodes.last_mut().expect("just pushed").hazard = Some(HAZARD_XER);
                        Some(node)
                    }
                    BinaryOperator::ShiftLeft => {
                        let shift = u8::try_from(constant_right?).ok().filter(|shift| *shift < 32)?;
                        let value = self.expression(left, generator)?;
                        Some(self.push(OpKind::Alu, 1, 1, vec![value], Template::ShiftLeftImmediate(shift)))
                    }
                    BinaryOperator::BitAnd => {
                        let mask = u32::try_from(constant_right.or(constant_left)?).ok()?;
                        let operand = if constant_right.is_some() { left } else { right };
                        let (begin, end) = contiguous_or_wrap_mask(mask)?;
                        let value = self.expression(operand, generator)?;
                        Some(self.push(OpKind::Alu, 1, 1, vec![value], Template::Mask(begin, end)))
                    }
                    BinaryOperator::BitOr => {
                        let constant = u32::try_from(constant_right.or(constant_left)?).ok()?;
                        let operand = if constant_right.is_some() { left } else { right };
                        let value = self.expression(operand, generator)?;
                        if constant <= 0xffff {
                            return Some(self.push(OpKind::Alu, 1, 1, vec![value], Template::OrImmediate(constant as u16)));
                        }
                        if constant & 0xffff == 0 {
                            return Some(self.push(
                                OpKind::Alu,
                                1,
                                1,
                                vec![value],
                                Template::OrImmediateShifted((constant >> 16) as u16),
                            ));
                        }
                        None
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

/// `(begin, end)` for an rlwinm-expressible mask: one contiguous run of ones,
/// possibly wrapping past bit 31 to bit 0 (PowerPC bit numbering).
fn contiguous_or_wrap_mask(mask: u32) -> Option<(u8, u8)> {
    if mask == 0 || mask == u32::MAX {
        return None;
    }
    let rotated = mask.rotate_right(mask.trailing_zeros() % 32);
    // After rotating the low run to the bottom, a single run is (2^n - 1)-shaped.
    if rotated.leading_zeros() + rotated.count_ones() + rotated.trailing_zeros() == 32 && rotated.trailing_zeros() == 0 {
        let begin = mask.leading_zeros() as u8;
        let end = (31 - mask.trailing_zeros()) as u8;
        if begin as u32 + mask.count_ones() - 1 == end as u32 {
            return Some((begin, end)); // contiguous, no wrap
        }
    }
    // Wrap mask: the complement is one contiguous interior run.
    let complement = !mask;
    if complement != 0 && mask.leading_zeros() == 0 && mask.trailing_zeros() == 0 {
        let run_start = complement.leading_zeros();
        let run_length = complement.count_ones();
        let expected = ((u64::MAX >> run_start) as u32) & !((u64::MAX >> (run_start + run_length)) as u32);
        if complement == expected {
            return Some(((run_start + run_length) as u8, run_start as u8 - 1));
        }
    }
    None
}

impl Generator {
    /// Leaf multi-store bodies through the measured models (see the module
    /// doc). Returns Ok(false) outside the validated envelope.
    pub(crate) fn try_dag_store_fill(&mut self, function: &Function) -> Compilation<bool> {
        if std::env::var("DAG_DEBUG").is_ok() {
            eprintln!("dag: entered for {}", function.name);
        }
        if !function.guards.is_empty()
            || !function.locals.is_empty()
            || function_makes_call(function)
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        // Void multi-store bodies, or INT-returning bodies with at least one
        // store (a pure computed return stays with the proven direct paths).
        let returns_int = matches!(function.return_type, Type::Int | Type::UnsignedInt);
        if !(function.return_type == Type::Void || (returns_int && function.return_expression.is_some())) {
            return Ok(false);
        }
        if self.behavior.global_addressing != GlobalAddressing::SmallData {
            return Ok(false);
        }
        let minimum_stores = if returns_int { 1 } else { 2 };
        if function.statements.len() < minimum_stores {
            return Ok(false);
        }
        // Parameters: ints and int pointers, all register-resident.
        let mut builder = Builder { nodes: Vec::new(), templates: Vec::new(), sources: Vec::new(), next_value: 0 };
        let mut params: Vec<(u32, u8)> = Vec::new();
        for parameter in &function.parameters {
            let register = match self.lookup_general(&parameter.name) {
                Some(register) => register,
                None => return Ok(false),
            };
            let value = builder.next_value;
            builder.next_value += 1;
            builder.sources.push((value, ValueSource::Parameter(register)));
            params.push((value, register));
        }
        // Statements: stores of recognizable int expressions to DISTINCT
        // small-data scalar globals.
        // TWO-PLUS bare-load store values serialize through r0 in mwcc (the
        // staging conflict, measured: lwz r0; stw; lwz r0; stw) — an extra
        // dependence the builder cannot derive yet. Defer that class.
        let bare_loads = function
            .statements
            .iter()
            .filter(|statement| matches!(statement, Statement::Store { value: Expression::Dereference { .. }, .. }))
            .count();
        if bare_loads >= 2 {
            return Ok(false);
        }
        let mut stored: Vec<&str> = Vec::new();
        for statement in &function.statements {
            let Statement::Store { target, value } = statement else { return Ok(false) };
            let Expression::Variable(global) = target else { return Ok(false) };
            if !matches!(self.globals.get(global.as_str()), Some(Type::Int | Type::UnsignedInt)) {
                return Ok(false);
            }
            if self.global_array_sizes.contains_key(global.as_str()) || stored.contains(&global.as_str()) {
                return Ok(false);
            }
            stored.push(global.as_str());
            let Some(value_id) = builder.expression(value, self) else { return Ok(false) };
            // Distinct globals do not alias: no group (the model reorders freely).
            let mut node = DagNode::new("", 1).kind(OpKind::Store);
            node.reads = vec![value_id];
            builder.nodes.push(node);
            builder.templates.push(Template::StoreGlobal(global.clone()));
        }
        // The RETURN chain: a consumerless value node — the register model
        // forces its result into r3 (the contracts' return mode).
        if returns_int {
            let return_expression = function.return_expression.as_ref().expect("gated");
            // A bare parameter/constant return has no chain to schedule —
            // those tails belong to the existing paths.
            if matches!(return_expression, Expression::Variable(_)) || constant_value(return_expression).is_some() {
                return Ok(false);
            }
            let before_return = builder.nodes.len();
            if builder.expression(return_expression, self).is_none() {
                return Ok(false);
            }
            // r0 CONTENTION (open model boundary): when a store chain ends in a
            // multiply and the return chain has intermediates, mwcc arbitrates
            // r0 by tenancy length (the mulli yields) — a lookahead the model
            // lacks. Defer the combination.
            let return_ops = builder.nodes.len() - before_return;
            let store_multiply_final = builder
                .templates
                .iter()
                .take(before_return)
                .any(|template| matches!(template, Template::MultiplyImmediate(_)));
            if return_ops >= 2 && store_multiply_final {
                // The legacy fall-through would emit this SEQUENTIALLY (wrong
                // bytes) — defer the whole function instead.
                return Err(Diagnostic::error(
                    "r0 contention between a multiply store chain and a computed return needs tenancy arbitration (roadmap)",
                ));
            }
        }
        // The PPC r0-as-zero rule: a value consumed as an addi source (or any
        // base field) must not live in r0 — mark producers so the register
        // model excludes it.
        for index in 0..builder.nodes.len() {
            let unsafe_reads: Vec<u32> = match &builder.templates[index] {
                Template::AddImmediate(_) => builder.nodes[index].reads.clone(),
                Template::LoadWord => builder.nodes[index].reads.clone(),
                _ => Vec::new(),
            };
            for read in unsafe_reads {
                if let ValueSource::Node(producer) = builder.value_of(read) {
                    builder.nodes[producer].forbid_r0 = true;
                }
            }
        }
        // -- the models take over --
        let order = linearize(&builder.nodes);
        if std::env::var("DAG_DEBUG").is_ok() {
            for (index, node) in builder.nodes.iter().enumerate() {
                eprintln!("node {index}: kind={:?} lat={} gate={} reads={:?} writes={:?}", node.kind, node.latency, node.gate_latency, node.reads, node.writes);
            }
            eprintln!("order: {order:?}");
        }
        let registers = assign_registers_v3(&builder.nodes, &order, &params);
        let register_of = |source: ValueSource, registers: &[Option<u8>]| -> Compilation<u8> {
            match source {
                ValueSource::Parameter(register) => Ok(register),
                ValueSource::Node(node) => registers[node]
                    .ok_or_else(|| Diagnostic::error("dag emitter: an unassigned value register (roadmap)")),
            }
        };
        for &node in &order {
            let operand = |index: usize| -> Compilation<u8> {
                register_of(builder.value_of(builder.nodes[node].reads[index]), &registers)
            };
            let destination = registers[node];
            let instruction = match &builder.templates[node] {
                Template::LoadImmediate(immediate) => Instruction::load_immediate(destination.expect("value node"), *immediate),
                Template::AddImmediate(immediate) => Instruction::AddImmediate {
                    d: destination.expect("value node"),
                    a: operand(0)?,
                    immediate: *immediate,
                },
                Template::Add => Instruction::Add { d: destination.expect("value node"), a: operand(0)?, b: operand(1)? },
                Template::Subtract => Instruction::SubtractFrom {
                    d: destination.expect("value node"),
                    a: operand(1)?,
                    b: operand(0)?,
                },
                Template::MultiplyImmediate(immediate) => Instruction::MultiplyImmediate {
                    d: destination.expect("value node"),
                    a: operand(0)?,
                    immediate: *immediate,
                },
                Template::ShiftLeftImmediate(shift) => Instruction::ShiftLeftImmediate {
                    a: destination.expect("value node"),
                    s: operand(0)?,
                    shift: *shift,
                },
                Template::ShiftRightAlgebraicImmediate(shift) => Instruction::ShiftRightAlgebraicImmediate {
                    a: destination.expect("value node"),
                    s: operand(0)?,
                    shift: *shift,
                },
                Template::OrImmediate(immediate) => Instruction::OrImmediate {
                    a: destination.expect("value node"),
                    s: operand(0)?,
                    immediate: *immediate,
                },
                Template::OrImmediateShifted(immediate) => Instruction::OrImmediateShifted {
                    a: destination.expect("value node"),
                    s: operand(0)?,
                    immediate: *immediate,
                },
                Template::Mask(begin, end) => Instruction::RotateAndMask {
                    a: destination.expect("value node"),
                    s: operand(0)?,
                    shift: 0,
                    begin: *begin,
                    end: *end,
                },
                Template::LoadWord => Instruction::LoadWord {
                    d: destination.expect("value node"),
                    a: operand(0)?,
                    offset: 0,
                },
                Template::StoreGlobal(global) => {
                    self.record_relocation(RelocationKind::EmbSda21, global);
                    Instruction::StoreWord { s: operand(0)?, a: 0, offset: 0 }
                }
            };
            self.output.instructions.push(instruction);
        }
        self.output.pre_scheduled = true;
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
