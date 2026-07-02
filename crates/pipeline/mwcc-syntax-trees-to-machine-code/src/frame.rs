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
        let guard_plan: Option<Vec<(&Expression, f64)>> = match function.guards.as_slice() {
            [] => None,
            guards @ ([_] | [_, _]) => {
                if !function.statements.is_empty() || function.return_type != Type::Double || function_makes_call(function) {
                    return Ok(false);
                }
                // The fall-through must be the FIRST double parameter, unwritten —
                // still live in f1, so the merge emits nothing.
                let Some(Expression::Variable(returned)) = &function.return_expression else { return Ok(false) };
                let first_float_parameter = function
                    .parameters
                    .iter()
                    .find(|parameter| matches!(parameter.parameter_type, Type::Float | Type::Double));
                if first_float_parameter.map(|parameter| parameter.name.as_str()) != Some(returned.as_str()) {
                    return Ok(false);
                }
                let mut plans = Vec::new();
                for GuardedReturn { condition, value } in guards {
                    let Expression::FloatLiteral(guard_value) = value else { return Ok(false) };
                    plans.push((condition, *guard_value));
                }
                Some(plans)
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

        // The guard tests classify once slots exist (their punned loads resolve
        // against them); only the FIRST guard's lis-staged constant hoists into
        // the prologue latency slot (a later guard materializes its lis inline —
        // measured). In a CHAIN every test must be an unmasked lis/addis compare
        // sharing one loaded word; other kinds are single-guard only.
        let guard_tests: Option<Vec<(GuardTest, f64)>> = match guard_plan {
            None => None,
            Some(plans) => {
                let mut tests = Vec::new();
                let chained = plans.len() > 1;
                for (condition, value) in plans {
                    let test = self.classify_guard_test(condition);
                    if chained
                        && !matches!(
                            test,
                            GuardTest::LisCompare { mask_top_bit: false, .. } | GuardTest::AddisZero { .. }
                        )
                    {
                        return Ok(false);
                    }
                    tests.push((test, value));
                }
                Some(tests)
            }
        };
        // Prologue: allocate the frame, save the link register if non-leaf, then
        // spill the address-taken parameters to their slots.
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -frame_size });
        if let Some((GuardTest::LisCompare { high, .. }, _)) = guard_tests.as_deref().and_then(<[_]>::first) {
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
        if let Some(tests) = guard_tests {
            let count = tests.len();
            let epilogue = self.fresh_label();
            let mut loaded: Option<i16> = None;
            for (index, (test, guard_value)) in tests.into_iter().enumerate() {
                let word = Eabi::general_result().number;
                let (options, condition_bit) = match test {
                    GuardTest::General(condition) => self.emit_condition_test(condition)?,
                    GuardTest::LisCompare { offset, mask_top_bit, options, condition_bit, high } => {
                        // The first guard's lis is hoisted into the prologue; a later
                        // guard materializes its constant inline (measured).
                        if index > 0 {
                            self.output.instructions.push(Instruction::load_immediate_shifted(GENERAL_SCRATCH, high));
                        }
                        if loaded != Some(offset) {
                            self.output.instructions.push(Instruction::LoadWord { d: word, a: 1, offset });
                        }
                        if mask_top_bit {
                            self.output.instructions.push(Instruction::ClearLeftImmediate { a: word, s: word, clear: 1 });
                            loaded = None;
                        } else {
                            loaded = Some(offset);
                        }
                        self.output.instructions.push(Instruction::CompareWord { a: word, b: GENERAL_SCRATCH });
                        (options, condition_bit)
                    }
                    GuardTest::AddisZero { offset, options, condition_bit, negated_high } => {
                        if loaded != Some(offset) {
                            self.output.instructions.push(Instruction::LoadWord { d: word, a: 1, offset });
                            loaded = Some(offset);
                        }
                        self.output.instructions.push(Instruction::AddImmediateShifted { d: GENERAL_SCRATCH, a: word, immediate: negated_high });
                        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: GENERAL_SCRATCH, immediate: 0 });
                        (options, condition_bit)
                    }
                    GuardTest::OrZero { left_offset, right_offset, mask_top_bit, options, condition_bit } => {
                        // Both words load first — the second fills the first's latency —
                        // then the mask, then the record-form or.
                        self.output.instructions.push(Instruction::LoadWord { d: word, a: 1, offset: left_offset });
                        self.output.instructions.push(Instruction::LoadWord { d: GENERAL_SCRATCH, a: 1, offset: right_offset });
                        if mask_top_bit {
                            self.output.instructions.push(Instruction::ClearLeftImmediate { a: word, s: word, clear: 1 });
                        }
                        self.output.instructions.push(Instruction::OrRecord { a: GENERAL_SCRATCH, s: word, b: GENERAL_SCRATCH });
                        loaded = if mask_top_bit { None } else { Some(left_offset) };
                        (options, condition_bit)
                    }
                };
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
        // Body statements, then the return value.
        for statement in &function.statements {
            self.emit_statement(statement)?;
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
