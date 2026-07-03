//! Frame-resident locals: a variable whose address is taken (via `&v`, or a
//! type-pun like `*(int*)&v`) cannot live in a register — it gets a stack-frame
//! slot. `&v` is `addi d, r1, slot`, reads/writes go to the slot, and a spilled
//! parameter is stored there in the prologue.

use std::collections::HashSet;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_versions::GlobalAddressing;
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, GuardedReturn, Pointee, Statement, Type};
use mwcc_target::Eabi;
use crate::analysis::*;
use crate::generator::*;

impl Generator {
    /// If the function takes the address of any variable, lower it with a stack
    /// frame: lay out a slot per address-taken parameter/local, spill the
    /// parameters in the prologue, and run the body against those slots. Returns
    /// whether this path took over the whole body.
    pub(crate) fn try_frame_resident(&mut self, function: &Function) -> Compilation<bool> {
        let address_taken = collect_address_taken(function);
        // A local array is frame-resident even without an explicit `&`: its name
        // decays to the slot address. Trigger this path for those too.
        let has_array_local = function.locals.iter().any(|local| local.array_length.is_some());
        if address_taken.is_empty() && !has_array_local {
            return Ok(false);
        }
        // This path handles a straight-line body (stores/calls) plus an optional
        // return — and ONE captured guard shape: a leaf double function whose guard
        // returns a float literal and whose fall-through returns the still-in-f1
        // parameter (`if (<punned test>) return 0.0; return x;` — measured: the
        // guard value falls INTO the shared epilogue; the skip branch targets it;
        // the fall-through emits nothing). Anything else defers.
        let guard_plan: Option<(Vec<(&Expression, FrameOutcome)>, FrameFall)> = match function.guards.as_slice() {
            [] => None,
            guards @ ([_] | [_, _]) => {
                if function.return_type != Type::Double || function_makes_call(function) {
                    return Ok(false);
                }
                // One leading `*ptr = C;` through an int-pointer PARAMETER may precede
                // the guards (frexp's `*eptr = 0`): its li hoists into the prologue
                // ahead of the guard's lis, and the store lands between the guard
                // word's load and its compare (measured). Anything else defers.
                match function.statements.as_slice() {
                    [] => {}
                    [Statement::Store { target: Expression::Dereference { pointer }, value }] => {
                        let Expression::Variable(pointer_name) = pointer.as_ref() else { return Ok(false) };
                        if !function.parameters.iter().any(|parameter| &parameter.name == pointer_name) {
                            return Ok(false);
                        }
                        if constant_value(value).and_then(|constant| i16::try_from(constant).ok()).is_none() {
                            return Ok(false);
                        }
                    }
                    _ => return Ok(false),
                }
                // Guard values and the fall-through may each be a float literal or
                // the FIRST double parameter (unwritten, still live in f1). Probed
                // combinations only: literal guards over a param fall-through (any
                // count), or ONE param-returning guard over a literal fall-through.
                let first_float_parameter = function
                    .parameters
                    .iter()
                    .find(|parameter| matches!(parameter.parameter_type, Type::Float | Type::Double))
                    .map(|parameter| parameter.name.as_str());
                let fall = match &function.return_expression {
                    Some(Expression::Variable(returned)) if first_float_parameter == Some(returned.as_str()) => FrameFall::Param,
                    Some(Expression::FloatLiteral(value)) => FrameFall::Literal(*value),
                    _ => return Ok(false),
                };
                let mut plans = Vec::new();
                for GuardedReturn { condition, value } in guards {
                    let outcome = match value {
                        Expression::FloatLiteral(guard_value) => FrameOutcome::Literal(*guard_value),
                        Expression::Variable(name) if first_float_parameter == Some(name.as_str()) => FrameOutcome::Param,
                        _ => return Ok(false),
                    };
                    plans.push((condition, outcome));
                }
                let all_literal = plans.iter().all(|(_, outcome)| matches!(outcome, FrameOutcome::Literal(_)));
                match (plans.len(), all_literal, fall) {
                    (_, true, FrameFall::Param) => {}
                    (1, false, FrameFall::Literal(_)) => {}
                    _ => return Ok(false),
                }
                Some((plans, fall))
            }
            _ => return Ok(false),
        };

        // Lay out a slot for each address-taken parameter (in argument order),
        // then each address-taken local, above the 8-byte linkage area.
        let mut offset: i16 = 8;
        let mut next_general = Eabi::FIRST_GENERAL_ARGUMENT;
        let mut next_float = Eabi::FIRST_FLOAT_ARGUMENT;
        for parameter in &function.parameters {
            let class = class_of(parameter.parameter_type)?;
            let register = match class {
                ValueClass::General => {
                    let register = next_general;
                    next_general += 1;
                    register
                }
                ValueClass::Float => {
                    let register = next_float;
                    next_float += 1;
                    register
                }
            };
            if address_taken.contains(parameter.name.as_str()) {
                let size = slot_size(parameter.parameter_type);
                offset = align_to(offset, slot_align(parameter.parameter_type));
                self.frame_slots.insert(
                    parameter.name.clone(),
                    FrameSlot { offset, class, size, parameter_register: Some(register), is_array: false },
                );
                offset += size as i16;
            }
        }
        for local in &function.locals {
            let is_array = local.array_length.is_some();
            if address_taken.contains(local.name.as_str()) || is_array {
                // Only an uninitialized local is modeled here (its value comes from a
                // store through the taken address, or — for an array — element stores).
                if local.initializer.is_some() {
                    return Ok(false);
                }
                let class = class_of(local.declared_type)?;
                // An array occupies `N * sizeof(element)` bytes — the element's true
                // width (1 for `char`), not the 4-byte spill slot a scalar uses. The
                // slot size field is a byte, so a larger array defers.
                let bytes = match local.array_length {
                    Some(length) => (local.declared_type.width() as u16 / 8) * length,
                    None => slot_size(local.declared_type) as u16,
                };
                if bytes > u8::MAX as u16 {
                    return Ok(false);
                }
                offset = align_to(offset, slot_align(local.declared_type));
                self.frame_slots.insert(
                    local.name.clone(),
                    FrameSlot { offset, class, size: bytes as u8, parameter_register: None, is_array },
                );
                offset += bytes as i16;
            }
        }

        // The frame is the linkage area plus the slots, rounded up to 16 bytes.
        let frame_size = (((offset as i32) + 15) / 16 * 16) as i16;
        let non_leaf = function_makes_call(function);
        self.non_leaf = non_leaf;
        self.frame_size = frame_size;

        // The leading store's pieces: the pointer parameter's register and a fresh
        // virtual for its li'd value (materialized in the prologue, stored after
        // the first guard word's load). Requires the first test to be the probed
        // lis-compare shape.
        let store_plan: Option<(u8, u8, i16)> = match (guard_plan.is_some(), function.statements.as_slice()) {
            (true, [Statement::Store { target: Expression::Dereference { pointer }, value }]) => {
                let Expression::Variable(pointer_name) = pointer.as_ref() else { return Ok(false) };
                let Some(register) = self.lookup_general(pointer_name) else { return Ok(false) };
                let Some(constant) = constant_value(value).and_then(|constant| i16::try_from(constant).ok()) else {
                    return Ok(false);
                };
                let value_home = self.fresh_virtual_general();
                Some((value_home, register, constant))
            }
            _ => None,
        };
        // The guard tests classify once slots exist (their punned loads resolve
        // against them); only the FIRST guard's lis-staged constant hoists into
        // the prologue latency slot (a later guard materializes its lis inline —
        // measured). In a CHAIN every test must be an unmasked lis/addis compare
        // sharing one loaded word; other kinds are single-guard only.
        let guard_tests: Option<(Vec<(Vec<GuardTest>, FrameOutcome)>, FrameFall)> = match guard_plan {
            None => None,
            Some((plans, fall)) => {
                let mut tests = Vec::new();
                let chained = plans.len() > 1;
                for (condition, value) in plans {
                    // `T1 || T2` inside ONE guard is a DISJUNCTION: the first test
                    // branches INTO the value block on TRUE, the second skips past
                    // it. Only a lone guard takes it (chains of disjunctions are
                    // unprobed), and only the two measured pairings emit.
                    let disjuncts: Vec<GuardTest> = match condition {
                        Expression::Binary { operator: BinaryOperator::LogicalOr, left, right } => {
                            if chained {
                                return Ok(false);
                            }
                            let first = self.classify_guard_test(left.as_ref());
                            let mut second = self.classify_guard_test(right.as_ref());
                            // The shared-word cmpwi form: a small compare in second
                            // position over the SAME unmasked word (measured g2).
                            if let (GuardTest::LisCompare { offset, mask_top_bit: false, .. }, GuardTest::General(condition)) =
                                (&first, &second)
                            {
                                if let Some(small @ GuardTest::SmallCompare { offset: small_offset, .. }) =
                                    self.classify_small_compare(condition)
                                {
                                    if small_offset == *offset {
                                        second = small;
                                    }
                                }
                            }
                            vec![first, second]
                        }
                        _ => vec![self.classify_guard_test(condition)],
                    };
                    if chained
                        && !matches!(
                            disjuncts.as_slice(),
                            [GuardTest::LisCompare { mask_top_bit: false, .. } | GuardTest::AddisZero { .. }]
                        )
                    {
                        return Ok(false);
                    }
                    if disjuncts.len() > 1 {
                        // The measured pairings: a lis compare, then EITHER an or.-zero
                        // over the SAME (offset, mask) word plus a second word, OR a
                        // second lis compare of the same unmasked word, OR a shared-word
                        // small compare.
                        let ok = matches!(
                            disjuncts.as_slice(),
                            [GuardTest::LisCompare { offset: o1, mask_top_bit: m1, .. },
                             GuardTest::OrZero { left_offset, mask_top_bit: m2, .. }]
                                if o1 == left_offset && m1 == m2
                        ) || matches!(
                            disjuncts.as_slice(),
                            [GuardTest::LisCompare { offset: o1, mask_top_bit: false, .. },
                             GuardTest::LisCompare { offset: o2, mask_top_bit: false, .. }]
                                if o1 == o2
                        ) || matches!(
                            disjuncts.as_slice(),
                            [GuardTest::LisCompare { mask_top_bit: false, .. }, GuardTest::SmallCompare { .. }]
                        );
                        if !ok {
                            return Ok(false);
                        }
                    }
                    tests.push((disjuncts, value));
                }
                Some((tests, fall))
            }
        };
        // Prologue: allocate the frame, save the link register if non-leaf, then
        // spill the address-taken parameters to their slots.
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -frame_size });
        // The trailing WRITEBACK BLOCK (no guards): `if (test) { x *= C; } return x;`
        // — the guard-style skip over a float-multiply written back to x's slot,
        // with the merge reloading x unconditionally (measured m1). The test's lis
        // hoists exactly like a guard's.
        let writeback_plan: Option<(GuardTest, i16, f64)> = if guard_tests.is_none() {
            match function.statements.as_slice() {
                [Statement::If { condition, then_body, else_body }] if else_body.is_empty() => {
                    let returned = match &function.return_expression {
                        Some(Expression::Variable(name)) => Some(name.as_str()),
                        _ => None,
                    };
                    let spilled_double = returned.and_then(|name| {
                        self.frame_slots
                            .get(name)
                            .filter(|slot| slot.class == ValueClass::Float && slot.size == 8 && slot.parameter_register.is_some())
                            .map(|slot| (name, slot.offset))
                    });
                    let assign = match then_body.as_slice() {
                        [Statement::Assign { name, value }] => Some((name.as_str(), value)),
                        _ => None,
                    };
                    match (spilled_double, assign) {
                        (Some((x, slot_offset)), Some((target, value))) if x == target => match value {
                            Expression::Binary { operator: BinaryOperator::Multiply, left, right } => {
                                let self_multiply = matches!(left.as_ref(), Expression::Variable(name) if name.as_str() == x);
                                match (self_multiply, right.as_ref()) {
                                    (true, Expression::FloatLiteral(constant)) => {
                                        match self.classify_guard_test(condition) {
                                            test @ GuardTest::LisCompare { .. } => Some((test, slot_offset, *constant)),
                                            _ => None,
                                        }
                                    }
                                    _ => None,
                                }
                            }
                            _ => None,
                        },
                        _ => None,
                    }
                }
                _ => None,
            }
        } else {
            None
        };
        let first_test = guard_tests
            .as_ref()
            .and_then(|(tests, _)| tests.first())
            .and_then(|(disjuncts, _)| disjuncts.first())
            .or(writeback_plan.as_ref().map(|(test, _, _)| test));
        if store_plan.is_some() && !matches!(first_test, Some(GuardTest::LisCompare { .. })) {
            return Ok(false); // the store's schedule is only measured against a lis-compare
        }
        if let Some((value_home, _, constant)) = store_plan {
            self.output.instructions.push(Instruction::load_immediate(value_home, constant));
        }
        if let Some(GuardTest::LisCompare { high, .. }) = first_test {
            self.output.instructions.push(Instruction::load_immediate_shifted(GENERAL_SCRATCH, *high));
        }
        if non_leaf {
            self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
            self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: frame_size + 4 });
        }
        for parameter in &function.parameters {
            if let Some(slot) = self.frame_slots.get(&parameter.name).copied() {
                if let Some(register) = slot.parameter_register {
                    self.output.instructions.push(spill_instruction(register, slot));
                }
            }
        }

        // The guard-chain shape: per guard — test, skip branch, value into f1 —
        // where a NON-final guard's value takes `b` to the shared epilogue and the
        // final one falls into it. One loaded word is shared down the chain
        // (`loaded` tracks what r3 holds); only unmasked words stay shared.
        if let Some((tests, fall)) = guard_tests {
            // The reversed form: ONE guard returning the parameter over a literal
            // fall-through. Plain: `test; b<skip> FALL; b EPI; FALL: lfd literal; EPI`
            // (g1 — the empty value block still gets its unconditional branch, no
            // condition inversion). Disjunction: the value block is a branch JOIN, so
            // `return x` RELOADS from the slot (`lfd f1,slot; b EPI` — g2).
            if let FrameFall::Literal(fall_value) = fall {
                let (disjuncts, _) = tests.into_iter().next().expect("gated: one guard");
                let epilogue = self.fresh_label();
                if disjuncts.len() > 1 {
                    let value_label = self.fresh_label();
                    let fall_label = self.fresh_label();
                    self.emit_frame_disjunction(&disjuncts, value_label, fall_label)?;
                    self.bind_label(value_label);
                    let slot = function
                        .parameters
                        .iter()
                        .find(|parameter| matches!(parameter.parameter_type, Type::Float | Type::Double))
                        .and_then(|parameter| self.frame_slots.get(&parameter.name))
                        .expect("gated: spilled double parameter");
                    self.output.instructions.push(Instruction::LoadFloatDouble { d: Eabi::float_result().number, a: 1, offset: slot.offset });
                    self.emit_branch_to(epilogue);
                    self.bind_label(fall_label);
                    self.evaluate(&Expression::FloatLiteral(fall_value), Type::Double, Eabi::float_result().number)?;
                    self.output.anonymous_label_bump += 3;
                } else {
                    let mut loaded: Option<(i16, u8)> = None;
                    let test = disjuncts.into_iter().next().expect("one disjunct");
                    let (options, condition_bit) = self.emit_frame_guard_test(test, 0, &mut loaded, store_plan)?;
                    let fall_label = self.fresh_label();
                    self.emit_branch_conditional_to(options, condition_bit, fall_label);
                    self.emit_branch_to(epilogue);
                    self.bind_label(fall_label);
                    self.evaluate(&Expression::FloatLiteral(fall_value), Type::Double, Eabi::float_result().number)?;
                    self.output.anonymous_label_bump += 2;
                }
                self.bind_label(epilogue);
                self.emit_epilogue_and_return();
                return Ok(true);
            }
            let count = tests.len();
            let epilogue = self.fresh_label();
            // The shared loaded word, as (offset, VIRTUAL register): the words are
            // virtuals now — the allocator reproduces r3/r4 from liveness here and
            // scales to frexp's r4/r5/r6 as more values go live (the convergence).
            let mut loaded: Option<(i16, u8)> = None;
            for (index, (disjuncts, outcome)) in tests.into_iter().enumerate() {
                let FrameOutcome::Literal(guard_value) = outcome else { unreachable!("gated: literal guards") };
                if disjuncts.len() > 1 {
                    let value_label = self.fresh_label();
                    self.emit_frame_disjunction(&disjuncts, value_label, epilogue)?;
                    self.bind_label(value_label);
                    self.evaluate(&Expression::FloatLiteral(guard_value), Type::Double, Eabi::float_result().number)?;
                    // A disjunction advances the label counter 3 — two tests sharing
                    // one value block (measured @N: real @8 vs @9 at +4).
                    self.output.anonymous_label_bump += 3;
                    continue;
                }
                let (options, condition_bit) =
                    self.emit_frame_guard_test(disjuncts.into_iter().next().expect("one disjunct"), index, &mut loaded, store_plan)?;
                if index + 1 == count {
                    self.emit_branch_conditional_to(options, condition_bit, epilogue);
                    self.evaluate(&Expression::FloatLiteral(guard_value), Type::Double, Eabi::float_result().number)?;
                } else {
                    let next = self.fresh_label();
                    self.emit_branch_conditional_to(options, condition_bit, next);
                    self.evaluate(&Expression::FloatLiteral(guard_value), Type::Double, Eabi::float_result().number)?;
                    self.emit_branch_to(epilogue);
                    self.bind_label(next);
                }
                // mwcc's internal label counter advances 2 per guard (measured @N).
                self.output.anonymous_label_bump += 2;
            }
            self.bind_label(epilogue);
            self.emit_epilogue_and_return();
            return Ok(true);
        }
        // The writeback block: guard-style skip over `x *= C` stored to the slot;
        // the merge falls into the return, which reloads (the slot is written).
        if let Some((test, slot_offset, constant)) = writeback_plan {
            let mut loaded: Option<(i16, u8)> = None;
            let (options, condition_bit) = self.emit_frame_guard_test(test, 0, &mut loaded, None)?;
            let merge = self.fresh_label();
            self.emit_branch_conditional_to(options, condition_bit, merge);
            let x_register = self
                .frame_slots
                .values()
                .find(|slot| slot.offset == slot_offset)
                .and_then(|slot| slot.parameter_register)
                .expect("gated: spilled parameter");
            self.load_double_constant(FLOAT_SCRATCH, constant.to_bits());
            self.output.instructions.push(Instruction::FloatMultiplyDouble { d: FLOAT_SCRATCH, a: x_register, c: FLOAT_SCRATCH });
            self.output.instructions.push(Instruction::StoreFloatDouble { s: FLOAT_SCRATCH, a: 1, offset: slot_offset });
            self.written_slots.insert(slot_offset);
            self.bind_label(merge);
            // The block advances the label counter like a guard (measured @N).
            self.output.anonymous_label_bump += 2;
        } else {
            // Body statements, then the return value.
            for statement in &function.statements {
                self.emit_statement(statement)?;
            }
        }
        if function.return_type != Type::Void {
            let result = match function.return_type {
                Type::Float | Type::Double => Eabi::float_result().number,
                _ => Eabi::general_result().number,
            };
            let return_expression = function
                .return_expression
                .as_ref()
                .ok_or_else(|| Diagnostic::error("a non-void function needs a return value"))?;
            self.evaluate(return_expression, function.return_type, result)?;
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// Fold a pointer expression of the form `(n +) (t*) &framevar` to the
    /// `(pointee, byte offset from r1)` it accesses, or `None` if it does not
    /// reduce to a frame-resident address. This is how a type-pun such as
    /// `*(1 + (int*)&x)` becomes a plain displacement load/store from `r1`.

    /// Emit one single-test frame guard's compare, returning the skip branch's
    /// (options, condition bit). `loaded` is the chain's shared-word tracker;
    /// `store_plan` is the leading `*ptr = C` store landing after the first load.
    fn emit_frame_guard_test(
        &mut self,
        test: GuardTest,
        index: usize,
        loaded: &mut Option<(i16, u8)>,
        store_plan: Option<(u8, u8, i16)>,
    ) -> Compilation<(u8, u8)> {
        let result = match test {
                    GuardTest::General(condition) => self.emit_condition_test(condition)?,
                    GuardTest::LisCompare { offset, mask_top_bit, options, condition_bit, high } => {
                        // The first guard's lis is hoisted into the prologue; a later
                        // guard materializes its constant inline (measured).
                        if index > 0 {
                            self.output.instructions.push(Instruction::load_immediate_shifted(GENERAL_SCRATCH, high));
                        }
                        let word = match *loaded {
                            Some((shared_offset, shared_word)) if shared_offset == offset => shared_word,
                            _ => {
                                let word = self.fresh_virtual_general();
                                self.output.instructions.push(Instruction::LoadWord { d: word, a: 1, offset });
                                word
                            }
                        };
                        // The leading store fills the load latency — BEFORE the mask
                        // (measured: lwz; stw; clrlwi; cmpw).
                        if index == 0 {
                            if let Some((value_home, pointer, _)) = store_plan {
                                self.output.instructions.push(Instruction::StoreWord { s: value_home, a: pointer, offset: 0 });
                            }
                        }
                        let word = if mask_top_bit {
                            // The masked value is a NEW value home (mwcc hands it the
                            // lowest register freed by that point — the die-at-definition
                            // reuse gives the same register back when nothing freed).
                            let masked = self.fresh_virtual_general();
                            self.output.instructions.push(Instruction::ClearLeftImmediate { a: masked, s: word, clear: 1 });
                            *loaded = None;
                            masked
                        } else {
                            *loaded = Some((offset, word));
                            word
                        };
                        self.output.instructions.push(Instruction::CompareWord { a: word, b: GENERAL_SCRATCH });
                        (options, condition_bit)
                    }
                    GuardTest::AddisZero { offset, options, condition_bit, negated_high } => {
                        let word = match *loaded {
                            Some((shared_offset, shared_word)) if shared_offset == offset => shared_word,
                            _ => {
                                let word = self.fresh_virtual_general();
                                self.output.instructions.push(Instruction::LoadWord { d: word, a: 1, offset });
                                *loaded = Some((offset, word));
                                word
                            }
                        };
                        self.output.instructions.push(Instruction::AddImmediateShifted { d: GENERAL_SCRATCH, a: word, immediate: negated_high });
                        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: GENERAL_SCRATCH, immediate: 0 });
                        (options, condition_bit)
                    }
                    GuardTest::OrZero { left_offset, right_offset, mask_top_bit, options, condition_bit } => {
                        // Both words load first — the second fills the first's latency —
                        // then the mask, then the record-form or. (The right word lives
                        // in r0 here: this single-test form frees it before the branch.)
                        let word = self.fresh_virtual_general();
                        self.output.instructions.push(Instruction::LoadWord { d: word, a: 1, offset: left_offset });
                        self.output.instructions.push(Instruction::LoadWord { d: GENERAL_SCRATCH, a: 1, offset: right_offset });
                        let word = if mask_top_bit {
                            let masked = self.fresh_virtual_general();
                            self.output.instructions.push(Instruction::ClearLeftImmediate { a: masked, s: word, clear: 1 });
                            masked
                        } else {
                            word
                        };
                        self.output.instructions.push(Instruction::OrRecord { a: GENERAL_SCRATCH, s: word, b: GENERAL_SCRATCH });
                        *loaded = if mask_top_bit { None } else { Some((left_offset, word)) };
                        (options, condition_bit)
                    }
                    GuardTest::SmallCompare { .. } => unreachable!("a small compare only pairs as a disjunction's second test"),
                };
        Ok(result)
    }

    /// Emit a two-test disjunction's loads, compares, and branches: the first test
    /// branches INTO `value_label` on TRUE, the second skips to `skip_target` when
    /// false. All loads come first (the or.-pairing's second word rides the first's
    /// load latency), the mask after (a fresh value home), then the tests.
    fn emit_frame_disjunction(&mut self, disjuncts: &[GuardTest], value_label: mwcc_vreg::Label, skip_target: mwcc_vreg::Label) -> Compilation<()> {
        match disjuncts {
            [GuardTest::LisCompare { offset, mask_top_bit, options, condition_bit, .. },
             GuardTest::OrZero { right_offset, options: or_options, condition_bit: or_bit, .. }] => {
                let word = self.fresh_virtual_general();
                let second_word = self.fresh_virtual_general();
                self.output.instructions.push(Instruction::LoadWord { d: word, a: 1, offset: *offset });
                self.output.instructions.push(Instruction::LoadWord { d: second_word, a: 1, offset: *right_offset });
                let word = if *mask_top_bit {
                    let masked = self.fresh_virtual_general();
                    self.output.instructions.push(Instruction::ClearLeftImmediate { a: masked, s: word, clear: 1 });
                    masked
                } else {
                    word
                };
                self.output.instructions.push(Instruction::CompareWord { a: word, b: GENERAL_SCRATCH });
                self.emit_branch_conditional_to(options ^ 8, *condition_bit, value_label);
                self.output.instructions.push(Instruction::OrRecord { a: GENERAL_SCRATCH, s: word, b: second_word });
                self.emit_branch_conditional_to(*or_options, *or_bit, skip_target);
            }
            [GuardTest::LisCompare { offset, options, condition_bit, .. },
             GuardTest::LisCompare { options: second_options, condition_bit: second_bit, high: second_high, .. }] => {
                let word = self.fresh_virtual_general();
                self.output.instructions.push(Instruction::LoadWord { d: word, a: 1, offset: *offset });
                self.output.instructions.push(Instruction::CompareWord { a: word, b: GENERAL_SCRATCH });
                self.emit_branch_conditional_to(options ^ 8, *condition_bit, value_label);
                self.output.instructions.push(Instruction::load_immediate_shifted(GENERAL_SCRATCH, *second_high));
                self.output.instructions.push(Instruction::CompareWord { a: word, b: GENERAL_SCRATCH });
                self.emit_branch_conditional_to(*second_options, *second_bit, skip_target);
            }
            [GuardTest::LisCompare { offset, options, condition_bit, .. },
             GuardTest::SmallCompare { constant, options: second_options, condition_bit: second_bit, .. }] => {
                // The shared-word cmpwi second test (measured g2).
                let word = self.fresh_virtual_general();
                self.output.instructions.push(Instruction::LoadWord { d: word, a: 1, offset: *offset });
                self.output.instructions.push(Instruction::CompareWord { a: word, b: GENERAL_SCRATCH });
                self.emit_branch_conditional_to(options ^ 8, *condition_bit, value_label);
                self.output.instructions.push(Instruction::CompareWordImmediate { a: word, immediate: *constant });
                self.emit_branch_conditional_to(*second_options, *second_bit, skip_target);
            }
            _ => unreachable!("gated at classification"),
        }
        Ok(())
    }

    /// The frexp FAMILY: punned int locals over a spilled double parameter and an
    /// int-pointer out-parameter, with a param-returning disjunction guard, a
    /// float-multiply writeback block that REASSIGNS the locals, and a closing
    /// arithmetic tail. The locals live as VIRTUALS: the block's reassignment
    /// redefines the same virtual, so the redefinition-spanning rule gives each
    /// ONE home across the diamond (merge agreement), and the allocator
    /// reproduces the captured r5/r4/r6 from liveness with eptr pinned in r3.
    /// Every ORDER below is measured (the 35-instruction s_frexp capture).
    pub(crate) fn try_frexp_family(&mut self, function: &Function) -> Compilation<bool> {
        // ---- recognition (any mismatch declines) ----
        if !function.guards.is_empty() || function.return_type != Type::Double || function_makes_call(function) {
            return Ok(false);
        }
        let [param_x, param_e] = function.parameters.as_slice() else { return Ok(false) };
        if param_x.parameter_type != Type::Double || !matches!(param_e.parameter_type, Type::Pointer(Pointee::Int)) {
            return Ok(false);
        }
        let (x, eptr) = (param_x.name.as_str(), param_e.name.as_str());
        let [local_hx, local_ix, local_lx] = function.locals.as_slice() else { return Ok(false) };
        if [local_hx, local_ix, local_lx].iter().any(|local| local.declared_type != Type::Int || local.array_length.is_some()) {
            return Ok(false);
        }
        let (hx, ix, lx) = (local_hx.name.as_str(), local_ix.name.as_str(), local_lx.name.as_str());
        // The locals may be initialized via declarations OR via leading assign
        // statements (`int hx, ix, lx; hx = …;` — the fdlibm style parses as
        // uninitialized locals plus assigns). Normalize to three value sources.
        let (hx_init, ix_init, lx_init, rest): (&Expression, &Expression, &Expression, &[Statement]) =
            match (&local_hx.initializer, &local_ix.initializer, &local_lx.initializer, function.statements.as_slice()) {
                (Some(hx_init), Some(ix_init), Some(lx_init), rest) => (hx_init, ix_init, lx_init, rest),
                (
                    None,
                    None,
                    None,
                    [Statement::Assign { name: a0, value: v0 }, Statement::Assign { name: a1, value: v1 }, Statement::Assign { name: a2, value: v2 }, rest @ ..],
                ) if a0 == hx && a1 == ix && a2 == lx => (v0, v1, v2, rest),
                _ => return Ok(false),
            };
        if pun_word_offset(hx_init, x) != Some(0) {
            return Ok(false);
        }
        if as_mask31_of(ix_init, hx) != Some(true) {
            return Ok(false);
        }
        if pun_word_offset(lx_init, x) != Some(4) {
            return Ok(false);
        }
        let [s0, s1, s2, s3, s4, s5] = rest else { return Ok(false) };
        // s0: *eptr = 0;
        if !matches!(s0, Statement::Store { target: Expression::Dereference { pointer }, value }
            if matches!(pointer.as_ref(), Expression::Variable(name) if name == eptr) && constant_value(value) == Some(0))
        {
            return Ok(false);
        }
        // s1: if (ix >= C1 || ((ix|lx) == 0)) return x;
        let Statement::If { condition, then_body, else_body } = s1 else { return Ok(false) };
        if !else_body.is_empty() || !matches!(then_body.as_slice(), [Statement::Return(Some(Expression::Variable(name)))] if name == x) {
            return Ok(false);
        }
        let Expression::Binary { operator: BinaryOperator::LogicalOr, left, right } = condition else { return Ok(false) };
        let Expression::Binary { operator: BinaryOperator::GreaterEqual, left: ge_left, right: ge_right } = left.as_ref() else {
            return Ok(false);
        };
        if !matches!(ge_left.as_ref(), Expression::Variable(name) if name == ix) {
            return Ok(false);
        }
        let Some(guard_high) = lis_able_high(ge_right) else { return Ok(false) };
        let Expression::Binary { operator: BinaryOperator::Equal, left: or_expr, right: zero } = right.as_ref() else {
            return Ok(false);
        };
        if constant_value(zero) != Some(0) {
            return Ok(false);
        }
        if !matches!(or_expr.as_ref(), Expression::Binary { operator: BinaryOperator::BitOr, left, right }
            if matches!(left.as_ref(), Expression::Variable(name) if name == ix)
                && matches!(right.as_ref(), Expression::Variable(name) if name == lx))
        {
            return Ok(false);
        }
        // s2: if (ix < C2) { x *= F; hx = __HI(x); ix = hx & 0x7fffffff; *eptr = C3; }
        let Statement::If { condition, then_body, else_body } = s2 else { return Ok(false) };
        if !else_body.is_empty() {
            return Ok(false);
        }
        let Expression::Binary { operator: BinaryOperator::Less, left: lt_left, right: lt_right } = condition else {
            return Ok(false);
        };
        if !matches!(lt_left.as_ref(), Expression::Variable(name) if name == ix) {
            return Ok(false);
        }
        let Some(block_high) = lis_able_high(lt_right) else { return Ok(false) };
        let [b0, b1, b2, b3] = then_body.as_slice() else { return Ok(false) };
        let Statement::Assign { name: b0_name, value: b0_value } = b0 else { return Ok(false) };
        let Expression::Binary { operator: BinaryOperator::Multiply, left: mul_left, right: mul_right } = b0_value else {
            return Ok(false);
        };
        if b0_name != x
            || !matches!(mul_left.as_ref(), Expression::Variable(name) if name == x)
        {
            return Ok(false);
        }
        let Expression::FloatLiteral(scale) = mul_right.as_ref() else { return Ok(false) };
        if !matches!(b1, Statement::Assign { name, value } if name == hx && pun_word_offset(value, x) == Some(0)) {
            return Ok(false);
        }
        if !matches!(b2, Statement::Assign { name, value } if name == ix && as_mask31_of(value, hx) == Some(true)) {
            return Ok(false);
        }
        let Statement::Store { target: b3_target, value: b3_value } = b3 else { return Ok(false) };
        if !matches!(b3_target, Expression::Dereference { pointer } if matches!(pointer.as_ref(), Expression::Variable(name) if name == eptr)) {
            return Ok(false);
        }
        let Some(store_constant) = constant_value(b3_value).and_then(|constant| i16::try_from(constant).ok()) else {
            return Ok(false);
        };
        // s3: *eptr += (ix >> C4) - C5;   (desugared: *eptr = *eptr + ((ix >> C4) - C5))
        let Statement::Store { target: s3_target, value: s3_value } = s3 else { return Ok(false) };
        if !matches!(s3_target, Expression::Dereference { pointer } if matches!(pointer.as_ref(), Expression::Variable(name) if name == eptr)) {
            return Ok(false);
        }
        let Expression::Binary { operator: BinaryOperator::Add, left: add_left, right: add_right } = s3_value else {
            return Ok(false);
        };
        if !matches!(add_left.as_ref(), Expression::Dereference { pointer } if matches!(pointer.as_ref(), Expression::Variable(name) if name == eptr)) {
            return Ok(false);
        }
        let Expression::Binary { operator: BinaryOperator::Subtract, left: shift_expr, right: bias } = add_right.as_ref() else {
            return Ok(false);
        };
        let Some(bias) = constant_value(bias).and_then(|constant| i16::try_from(constant).ok()) else { return Ok(false) };
        let Expression::Binary { operator: BinaryOperator::ShiftRight, left: shift_left, right: shift_amount } = shift_expr.as_ref() else {
            return Ok(false);
        };
        if !matches!(shift_left.as_ref(), Expression::Variable(name) if name == ix) {
            return Ok(false);
        }
        let Some(shift) = constant_value(shift_amount).and_then(|constant| u8::try_from(constant).ok()).filter(|shift| *shift < 32) else {
            return Ok(false);
        };
        // s4: hx = (hx & M1) | M2;  (M1 an rlwinm mask, M2 an oris-able immediate)
        let Statement::Assign { name: s4_name, value: s4_value } = s4 else { return Ok(false) };
        if s4_name != hx {
            return Ok(false);
        }
        let Expression::Binary { operator: BinaryOperator::BitOr, left: and_expr, right: or_constant } = s4_value else {
            return Ok(false);
        };
        let Some(or_high) = lis_able_high(or_constant) else { return Ok(false) };
        let Expression::Binary { operator: BinaryOperator::BitAnd, left: and_left, right: and_right } = and_expr.as_ref() else {
            return Ok(false);
        };
        if !matches!(and_left.as_ref(), Expression::Variable(name) if name == hx) {
            return Ok(false);
        }
        let Some((mask_begin, mask_end)) = constant_value(and_right).and_then(|constant| u32::try_from(constant).ok()).and_then(wrap_mask) else {
            return Ok(false);
        };
        // s5: *(int*)&x = hx;
        let Statement::Store { target: s5_target, value: s5_value } = s5 else { return Ok(false) };
        if pun_word_offset_target(s5_target, x) != Some(0) || !matches!(s5_value, Expression::Variable(name) if name == hx) {
            return Ok(false);
        }
        if !matches!(&function.return_expression, Some(Expression::Variable(name)) if name == x) {
            return Ok(false);
        }
        let Some(eptr_register) = self.lookup_general(eptr) else { return Ok(false) };

        // ---- emission (the measured 35-instruction schedule; registers from the allocator) ----
        const SLOT: i16 = 8;
        self.frame_slots.insert(
            param_x.name.clone(),
            FrameSlot { offset: SLOT, class: ValueClass::Float, size: 8, parameter_register: Some(Eabi::FIRST_FLOAT_ARGUMENT), is_array: false },
        );
        self.frame_size = 16;
        self.non_leaf = false;
        let float_result = Eabi::float_result().number;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        let value_zero = self.fresh_virtual_general();
        self.output.instructions.push(Instruction::load_immediate(value_zero, 0));
        self.output.instructions.push(Instruction::load_immediate_shifted(GENERAL_SCRATCH, guard_high));
        self.output.instructions.push(Instruction::StoreFloatDouble { s: Eabi::FIRST_FLOAT_ARGUMENT, a: 1, offset: SLOT });
        let virtual_hx = self.fresh_virtual_general();
        self.output.instructions.push(Instruction::LoadWord { d: virtual_hx, a: 1, offset: SLOT });
        self.output.instructions.push(Instruction::StoreWord { s: value_zero, a: eptr_register, offset: 0 });
        let virtual_ix = self.fresh_virtual_general();
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: virtual_ix, s: virtual_hx, clear: 1 });
        let virtual_lx = self.fresh_virtual_general();
        self.output.instructions.push(Instruction::LoadWord { d: virtual_lx, a: 1, offset: SLOT + 4 });
        self.output.instructions.push(Instruction::CompareWord { a: virtual_ix, b: GENERAL_SCRATCH });
        let value_label = self.fresh_label();
        let skip_label = self.fresh_label();
        let merge = self.fresh_label();
        let epilogue = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, value_label); // bge: >= is TRUE, into the value
        self.output.instructions.push(Instruction::OrRecord { a: GENERAL_SCRATCH, s: virtual_ix, b: virtual_lx });
        self.emit_branch_conditional_to(4, 2, skip_label); // bne: (ix|lx) != 0 skips the value
        self.bind_label(value_label);
        self.output.instructions.push(Instruction::LoadFloatDouble { d: float_result, a: 1, offset: SLOT });
        self.emit_branch_to(epilogue);
        self.bind_label(skip_label);
        self.output.instructions.push(Instruction::load_immediate_shifted(GENERAL_SCRATCH, block_high));
        self.output.instructions.push(Instruction::CompareWord { a: virtual_ix, b: GENERAL_SCRATCH });
        self.emit_branch_conditional_to(4, 0, merge); // bge: ix < C2 is FALSE, over the block
        self.load_double_constant(FLOAT_SCRATCH, scale.to_bits());
        self.output.instructions.push(Instruction::load_immediate(GENERAL_SCRATCH, store_constant));
        self.output.instructions.push(Instruction::StoreWord { s: GENERAL_SCRATCH, a: eptr_register, offset: 0 });
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: FLOAT_SCRATCH, a: Eabi::FIRST_FLOAT_ARGUMENT, c: FLOAT_SCRATCH });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: FLOAT_SCRATCH, a: 1, offset: SLOT });
        self.output.instructions.push(Instruction::LoadWord { d: virtual_hx, a: 1, offset: SLOT });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: virtual_ix, s: virtual_hx, clear: 1 });
        self.bind_label(merge);
        self.output.instructions.push(Instruction::RotateAndMask { a: GENERAL_SCRATCH, s: virtual_hx, shift: 0, begin: mask_begin, end: mask_end });
        let virtual_exponent = self.fresh_virtual_general();
        self.output.instructions.push(Instruction::LoadWord { d: virtual_exponent, a: eptr_register, offset: 0 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: virtual_ix, s: virtual_ix, shift });
        self.output.instructions.push(Instruction::OrImmediateShifted { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, immediate: or_high as u16 });
        self.output.instructions.push(Instruction::StoreWord { s: GENERAL_SCRATCH, a: 1, offset: SLOT });
        self.output.instructions.push(Instruction::Add { d: virtual_ix, a: virtual_ix, b: virtual_exponent });
        self.output.instructions.push(Instruction::AddImmediate { d: GENERAL_SCRATCH, a: virtual_ix, immediate: -bias });
        self.output.instructions.push(Instruction::StoreWord { s: GENERAL_SCRATCH, a: eptr_register, offset: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: float_result, a: 1, offset: SLOT });
        self.bind_label(epilogue);
        self.output.anonymous_label_bump += 6;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// Classify a frame-guard condition (see [`GuardTest`]). Frame slots must
    /// already be laid out — the punned load resolves against them.
    fn classify_guard_test<'a>(&self, condition: &'a Expression) -> GuardTest<'a> {
        if let Expression::Binary { operator, left, right } = condition {
            // `(a | b) ==/!= 0` over two punned words: the record-form or.
            if constant_value(right) == Some(0) && matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual) {
                if let Expression::Binary { operator: BinaryOperator::BitOr, left: or_left, right: or_right } = left.as_ref() {
                    let (left_word, mask_top_bit) = match or_left.as_ref() {
                        Expression::Binary { operator: BinaryOperator::BitAnd, left: inner, right: mask }
                            if constant_value(mask) == Some(0x7fff_ffff) => (inner.as_ref(), true),
                        other => (other, false),
                    };
                    if let (Expression::Dereference { pointer: left_pointer }, Expression::Dereference { pointer: right_pointer }) =
                        (left_word, or_right.as_ref())
                    {
                        if let (Some((Pointee::Int, left_offset)), Some((Pointee::Int, right_offset))) =
                            (self.resolve_frame_pointer(left_pointer), self.resolve_frame_pointer(right_pointer))
                        {
                            let (options, condition_bit) = signed_skip_when_false(*operator).expect("eq/ne mapped");
                            return GuardTest::OrZero { left_offset, right_offset, mask_top_bit, options, condition_bit };
                        }
                    }
                }
            }
            if let Some(constant) = constant_value(right) {
                let lis_able = i16::try_from(constant).is_err() && (constant & 0xffff) == 0 && u32::try_from(constant).is_ok();
                if lis_able {
                    let (word, mask_top_bit) = match left.as_ref() {
                        Expression::Binary { operator: BinaryOperator::BitAnd, left: inner, right: mask }
                            if constant_value(mask) == Some(0x7fff_ffff) => (inner.as_ref(), true),
                        other => (other, false),
                    };
                    if let Expression::Dereference { pointer } = word {
                        if let Some((Pointee::Int, offset)) = self.resolve_frame_pointer(pointer) {
                            // Equality folds the constant as `addis -HI; cmplwi 0` (no
                            // mask form measured); relations stage it with a hoisted lis.
                            if matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual) && !mask_top_bit {
                                let (options, condition_bit) = signed_skip_when_false(*operator).expect("eq/ne mapped");
                                return GuardTest::AddisZero { offset, options, condition_bit, negated_high: -((constant >> 16) as i16) };
                            }
                            if !matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual) {
                                if let Some((options, condition_bit)) = signed_skip_when_false(*operator) {
                                    return GuardTest::LisCompare { offset, mask_top_bit, options, condition_bit, high: (constant >> 16) as i16 };
                                }
                            }
                        }
                    }
                }
            }
        }
        GuardTest::General(condition)
    }

    /// A cmpwi-range compare over a bare punned word — only meaningful as a
    /// disjunction's SECOND test, where it reuses the shared loaded word
    /// (`cmpwi r3,C` — measured on g2). A lone small compare stays General
    /// (staging through r0), so this is a separate, pairing-time classification.
    fn classify_small_compare<'a>(&self, condition: &'a Expression) -> Option<GuardTest<'a>> {
        let Expression::Binary { operator, left, right } = condition else { return None };
        let constant = constant_value(right).and_then(|constant| i16::try_from(constant).ok())?;
        let Expression::Dereference { pointer } = left.as_ref() else { return None };
        let (Pointee::Int, offset) = self.resolve_frame_pointer(pointer)? else { return None };
        let (options, condition_bit) = signed_skip_when_false(*operator)?;
        Some(GuardTest::SmallCompare { offset, constant, options, condition_bit })
    }

    pub(crate) fn resolve_frame_pointer(&self, pointer: &Expression) -> Option<(Pointee, i16)> {
        match pointer {
            Expression::AddressOf { operand } => {
                let name = match operand.as_ref() {
                    Expression::Variable(name) => name,
                    _ => return None,
                };
                let slot = self.frame_slots.get(name)?;
                let pointee = match (slot.class, slot.size) {
                    (ValueClass::Float, 8) => Pointee::Double,
                    (ValueClass::Float, _) => Pointee::Float,
                    _ => Pointee::Int,
                };
                Some((pointee, slot.offset))
            }
            // A cast retargets the pointee; the address is unchanged.
            Expression::Cast { target_type: Type::Pointer(pointee), operand } => {
                let (_, offset) = self.resolve_frame_pointer(operand)?;
                Some((*pointee, offset))
            }
            // Pointer arithmetic scales the integer addend by the pointee size.
            Expression::Binary { operator: BinaryOperator::Add, left, right } => {
                if let (Some((pointee, offset)), Some(count)) = (self.resolve_frame_pointer(left), constant_value(right)) {
                    return Some((pointee, offset + count as i16 * pointee.size() as i16));
                }
                if let (Some((pointee, offset)), Some(count)) = (self.resolve_frame_pointer(right), constant_value(left)) {
                    return Some((pointee, offset + count as i16 * pointee.size() as i16));
                }
                None
            }
            _ => None,
        }
    }

    /// `&operand` into a register: the address of a frame-resident variable is
    /// `addi d, r1, slot`.
    pub(crate) fn emit_address_of(&mut self, operand: &Expression, destination: u8) -> Compilation<()> {
        if let Expression::Variable(name) = operand {
            if let Some(slot) = self.frame_slots.get(name) {
                let offset = slot.offset;
                self.output.instructions.push(Instruction::AddImmediate { d: destination, a: 1, immediate: offset });
                return Ok(());
            }
            // The address of a data global. Under small-data this is `addi d,r13,ga@sda21`
            // — the EMB_SDA21 relocation (the addi counterpart of the SDA value load),
            // encoded as `addi d,0,0` pre-link.
            if !self.locations.contains_key(name)
                && self.globals.contains_key(name.as_str())
                && self.behavior.global_addressing == GlobalAddressing::SmallData
            {
                self.record_relocation(RelocationKind::EmbSda21, name);
                self.output.instructions.push(Instruction::AddImmediate { d: destination, a: 0, immediate: 0 });
                return Ok(());
            }
        }
        if let Expression::Index { base, index } = operand {
            // `&a[i]` for a file-scope ARRAY global is the element ADDRESS `&a + i*size` (an
            // address computation), NOT the pointer arithmetic below — `a` is an array, so
            // `load(a)+i` would be wrong bytes. Route it to the array-base path.
            if let Expression::Variable(name) = base.as_ref() {
                if let Some(&total_size) = self.global_array_sizes.get(name.as_str()) {
                    return self.emit_global_array_element_address(name, total_size, index, destination);
                }
            }
            // `&p[i]` for a POINTER base is the element address `p + i` — the same pointer
            // arithmetic as `p + i`, scaling the index by the pointee size.
            let address = Expression::Binary {
                operator: mwcc_syntax_trees::BinaryOperator::Add,
                left: base.clone(),
                right: index.clone(),
            };
            return self.evaluate_general(&address, destination);
        }
        // `&*p` is just `p`.
        if let Expression::Dereference { pointer } = operand {
            return self.evaluate_general(pointer, destination);
        }
        if let Expression::Member { base, offset, index_stride: None, .. } = operand {
            if let Expression::Variable(name) = base.as_ref() {
                // `&g.field` where `g` is a file-scope struct VALUE global: the field address
                // `&g + offset` (an address computation), like `&a[i]` — NOT `load(g)+offset`.
                if !self.locations.contains_key(name.as_str()) {
                    if let Some(Type::Struct { size, .. }) = self.globals.get(name.as_str()).copied() {
                        return self.emit_global_struct_member_address(name, size as u32, *offset, destination);
                    }
                } else {
                    // `&p->field` where `p` is a register-resident struct POINTER: the pointer value
                    // plus the member offset (`addi dest,p,offset`, or `mr` at offset 0) — the same
                    // shape as the `MemberAddress` value path. `general_register_of` errors (so the
                    // whole address-of defers) when `name` is not a register-resident integer/pointer
                    // — e.g. a frame-resident struct VALUE — so `&s.field` stays deferred, not wrong.
                    let base_register = self.general_register_of(name)?;
                    if *offset == 0 {
                        if base_register != destination {
                            self.output.instructions.push(Instruction::move_register(destination, base_register));
                        }
                    } else {
                        let offset = i16::try_from(*offset).map_err(|_| Diagnostic::error("member address offset out of range (roadmap)"))?;
                        self.output.instructions.push(Instruction::AddImmediate { d: destination, a: base_register, immediate: offset });
                    }
                    return Ok(());
                }
            }
        }
        Err(Diagnostic::error("address-of a non-frame-resident lvalue is not supported yet (roadmap)"))
    }
}

/// The byte size of a variable's stack slot.
fn slot_size(declared: Type) -> u8 {
    match declared {
        Type::Double => 8,
        // A struct value occupies its full byte size on the stack.
        Type::Struct { size, .. } => size as u8,
        _ => 4,
    }
}

/// The stack alignment of a frame slot: a scalar aligns to its size, a struct to
/// its own (member) alignment rather than its total size.
fn slot_align(declared: Type) -> u8 {
    match declared {
        Type::Struct { align, .. } => align,
        other => slot_size(other),
    }
}

/// Round `offset` up to a multiple of `align`.
fn align_to(offset: i16, align: u8) -> i16 {
    let align = align as i16;
    (offset + align - 1) / align * align
}

/// The store that spills a parameter register to its frame slot.
fn spill_instruction(register: u8, slot: FrameSlot) -> Instruction {
    match (slot.class, slot.size) {
        (ValueClass::Float, 8) => Instruction::StoreFloatDouble { s: register, a: 1, offset: slot.offset },
        (ValueClass::Float, _) => Instruction::StoreFloatSingle { s: register, a: 1, offset: slot.offset },
        _ => Instruction::StoreWord { s: register, a: 1, offset: slot.offset },
    }
}

/// The set of variable names whose address is taken anywhere in the function.
/// How a single frame-guard's condition is emitted.
enum GuardTest<'a> {
    /// The generic condition emitter (small-immediate compares: `lwz r0; cmpwi`).
    General(&'a Expression),
    /// `<punned word> [& 0x7fffffff] CMP <lis-able constant>` — measured: the
    /// constant's `lis r0,HI` is HOISTED into the prologue latency slot (between
    /// `stwu` and the spill), the word loads into r3 (r0 is taken), the mask
    /// folds in place (`clrlwi r3,r3,1`), and a register `cmpw r3,r0` feeds the
    /// skip branch.
    LisCompare { offset: i16, mask_top_bit: bool, options: u8, condition_bit: u8, high: i16 },
    /// `<punned word> ==/!= <lis-able constant>` — measured: no lis at all;
    /// `addis r0,r3,-HI` folds the subtraction, then `cmplwi r0,0` feeds beq/bne.
    AddisZero { offset: i16, options: u8, condition_bit: u8, negated_high: i16 },
    /// `(<punned word> [& 0x7fffffff] | <punned word>) ==/!= 0` — the record-form
    /// OR: both words load (left to r3, right to r0 — the second load fills the
    /// first's latency, the mask following BOTH), then `or. r0,r3,r0` sets CR0
    /// with no separate compare.
    OrZero { left_offset: i16, right_offset: i16, mask_top_bit: bool, options: u8, condition_bit: u8 },
    /// `<punned word> CMP <cmpwi-range constant>` — only as a disjunction's SECOND
    /// test, reusing the shared loaded word (`cmpwi r3,C` — measured on g2).
    SmallCompare { offset: i16, constant: i16, options: u8, condition_bit: u8 },
}

/// What a frame guard returns when taken.
#[derive(Clone, Copy)]
enum FrameOutcome {
    Literal(f64),
    /// The double parameter itself: nothing when the value block falls in, an
    /// `lfd` slot reload when it is a branch JOIN target (measured g1 vs g2).
    Param,
}

/// What the fall-through return is.
#[derive(Clone, Copy)]
enum FrameFall {
    Param,
    Literal(f64),
}

/// The skip-when-false branch (options, CR bit) for a SIGNED compare — the
/// branch taken when the guard condition does NOT hold. LT=0, GT=1, EQ=2;
/// options 12 = branch-if-set, 4 = branch-if-clear.
fn signed_skip_when_false(operator: BinaryOperator) -> Option<(u8, u8)> {
    match operator {
        BinaryOperator::GreaterEqual => Some((12, 0)), // skip on LT
        BinaryOperator::Less => Some((4, 0)),          // skip on !LT
        BinaryOperator::LessEqual => Some((12, 1)),    // skip on GT
        BinaryOperator::Greater => Some((4, 1)),       // skip on !GT
        BinaryOperator::Equal => Some((4, 2)),         // skip on !EQ
        BinaryOperator::NotEqual => Some((12, 2)),     // skip on EQ
        _ => None,
    }
}

/// The word offset a punned READ of `&variable` accesses: `*(int*)&x` is 0,
/// `*(1+(int*)&x)` (either operand order) is 4. `None` for anything else.
pub(crate) fn pun_word_offset_pub(expression: &Expression, variable: &str) -> Option<i16> {
    pun_word_offset(expression, variable)
}

fn pun_word_offset(expression: &Expression, variable: &str) -> Option<i16> {
    // The array-index macro form `((int*)&x)[K]` (fminmaxdim's __HI/__LO).
    if let Expression::Index { base, index } = expression {
        let is_cast_address = matches!(base.as_ref(),
            Expression::Cast { target_type: Type::Pointer(Pointee::Int), operand }
                if matches!(operand.as_ref(), Expression::AddressOf { operand }
                    if matches!(operand.as_ref(), Expression::Variable(name) if name == variable)));
        if is_cast_address {
            return match constant_value(index) {
                Some(0) => Some(0),
                Some(1) => Some(4),
                _ => None,
            };
        }
    }
    let Expression::Dereference { pointer } = expression else { return None };
    pun_pointer_offset(pointer, variable)
}

/// The same for a store TARGET.
fn pun_word_offset_target(target: &Expression, variable: &str) -> Option<i16> {
    let Expression::Dereference { pointer } = target else { return None };
    pun_pointer_offset(pointer, variable)
}

fn pun_pointer_offset(pointer: &Expression, variable: &str) -> Option<i16> {
    let is_cast_address = |expression: &Expression| {
        matches!(expression, Expression::Cast { target_type: Type::Pointer(Pointee::Int), operand }
            if matches!(operand.as_ref(), Expression::AddressOf { operand }
                if matches!(operand.as_ref(), Expression::Variable(name) if name == variable)))
    };
    if is_cast_address(pointer) {
        return Some(0);
    }
    if let Expression::Binary { operator: BinaryOperator::Add, left, right } = pointer {
        if constant_value(left) == Some(1) && is_cast_address(right) {
            return Some(4);
        }
        if constant_value(right) == Some(1) && is_cast_address(left) {
            return Some(4);
        }
    }
    None
}

/// `Some(true)` when the expression is `name & 0x7fffffff` (either order).
fn as_mask31_of(expression: &Expression, name: &str) -> Option<bool> {
    let Expression::Binary { operator: BinaryOperator::BitAnd, left, right } = expression else { return None };
    let is_name = |expression: &Expression| matches!(expression, Expression::Variable(read) if read == name);
    let is_mask = |expression: &Expression| constant_value(expression) == Some(0x7fff_ffff);
    Some((is_name(left) && is_mask(right)) || (is_mask(left) && is_name(right)))
}

/// The high half of a lis-able constant (low half zero, beyond the cmpwi range).
fn lis_able_high(expression: &Expression) -> Option<i16> {
    let constant = constant_value(expression)?;
    (i16::try_from(constant).is_err() && (constant & 0xffff) == 0 && u32::try_from(constant).is_ok())
        .then(|| (constant >> 16) as i16)
}

/// The (begin, end) of an rlwinm WRAP mask (a contiguous run of ones wrapping
/// past bit 31 to bit 0), e.g. 0x800fffff -> (12, 0). `None` for other masks.
fn wrap_mask(mask: u32) -> Option<(u8, u8)> {
    // A wrap mask's COMPLEMENT is one contiguous run of ones not touching bit 0
    // or bit 31 (in PowerPC bit order: mask begin > end).
    let complement = !mask;
    if complement == 0 || mask.leading_zeros() > 0 || mask.trailing_zeros() > 0 {
        return None;
    }
    let run_start = complement.leading_zeros(); // first PPC bit of the zero run
    let run_length = complement.count_ones();
    if complement != (u32::MAX >> run_start) - if run_start + run_length == 32 { 0 } else { u32::MAX >> (run_start + run_length) } {
        return None;
    }
    Some(((run_start + run_length) as u8, run_start as u8 - 1))
}

pub(crate) fn collect_address_taken(function: &Function) -> HashSet<String> {
    let mut names = HashSet::new();
    for statement in &function.statements {
        walk_statement(statement, &mut names);
    }
    // A local INITIALIZER taking an address (`int hx = *(int*)&x;`) forces the
    // addressed variable frame-resident just as a statement would — value tracking
    // substitutes the initializer into the uses, where the slot must exist.
    for local in &function.locals {
        if let Some(initializer) = &local.initializer {
            walk(initializer, &mut names);
        }
    }
    if let Some(expression) = &function.return_expression {
        walk(expression, &mut names);
    }
    for GuardedReturn { condition, value } in &function.guards {
        walk(condition, &mut names);
        walk(value, &mut names);
    }
    // Only a parameter or local can be frame-resident. `&global` materializes the
    // global's address with a relocation and needs no stack slot, so it must not force
    // a frame (a non-empty set here suppresses the leaf no-frame path).
    let local_names: HashSet<&str> = function.parameters.iter().map(|parameter| parameter.name.as_str())
        .chain(function.locals.iter().map(|local| local.name.as_str()))
        .collect();
    names.retain(|name| local_names.contains(name.as_str()));
    names
}

/// Record `&variable` occurrences within a statement (recursing into if-blocks).
fn walk_statement(statement: &Statement, names: &mut HashSet<String>) {
    match statement {
        Statement::Break | Statement::Continue | Statement::Goto(_) | Statement::Label(_) => {}
        Statement::Store { target, value } => {
            walk(target, names);
            walk(value, names);
        }
        Statement::Expression(expression) => walk(expression, names),
        Statement::Assign { value, .. } => walk(value, names),
        Statement::Switch { scrutinee, .. } => walk(scrutinee, names),
        Statement::If { condition, then_body, else_body } => {
            walk(condition, names);
            for statement in then_body.iter().chain(else_body) {
                walk_statement(statement, names);
            }
        }
        Statement::Loop { initializer, condition, step, body, .. } => {
            for expression in initializer.iter().chain(condition).chain(step) {
                walk(expression, names);
            }
            for statement in body {
                walk_statement(statement, names);
            }
        }
        Statement::Return(value) => {
            if let Some(value) = value {
                walk(value, names);
            }
        }
    }
}

/// Record `&variable` occurrences within `expression`.
fn walk(expression: &Expression, names: &mut HashSet<String>) {
    match expression {
        Expression::AggregateLiteral(_) => {}
        Expression::PostStep { target, .. } => walk(target, names),
        // A string literal takes no `&variable` of its own.
        Expression::StringLiteral(_) => {}
        Expression::AddressOf { operand } => {
            if let Expression::Variable(name) = operand.as_ref() {
                names.insert(name.clone());
            }
            walk(operand, names);
        }
        Expression::Binary { left, right, .. } => {
            walk(left, names);
            walk(right, names);
        }
        Expression::Comma { left, right } => {
            walk(left, names);
            walk(right, names);
        }
        Expression::Unary { operand, .. } => walk(operand, names),
        Expression::Conditional { condition, when_true, when_false } => {
            walk(condition, names);
            walk(when_true, names);
            walk(when_false, names);
        }
        Expression::Cast { operand, .. } => walk(operand, names),
        Expression::Dereference { pointer } => walk(pointer, names),
        Expression::Index { base, index } => {
            walk(base, names);
            walk(index, names);
        }
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => walk(base, names),
        Expression::Assign { target, value } => {
            walk(target, names);
            walk(value, names);
        }
        Expression::Call { arguments, .. } => {
            for argument in arguments {
                walk(argument, names);
            }
        }
        Expression::Variable(_) | Expression::IntegerLiteral(_) | Expression::FloatLiteral(_) => {}
    }
}
