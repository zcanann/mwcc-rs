//! The PUNNED-GUARD prefix arm (try_punned_guard_float_return): the
//! __HI(x) frame spill, the nested fctiwz early return, the k_cos
//! if-form dispatch.

use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, Type};
use crate::generator::*;

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
            section: None,
            asm_body: None,
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
            self.float.reload_x = Some(8);
        }
        // The preserved ix resolves in the dual's condition test through a
        // temporary location at the prefix's compare register.
        if let Some((high, low)) = ix_dual_big {
            self.float.dual_compare = Some((high, low, target_register));
        }
        if let Some(mut payload) = composition {
            payload.ix_register = target_register;
            payload.addis_target = load_register;
            self.float.else_composition = Some(payload);
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
        self.float.reload_x = None;
        self.float.dual_compare = None;
        self.float.else_composition = None;
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

