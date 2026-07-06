//! Punned-double SHIFT writeback: unsigned-shift punned locals (i = C >> j0) writeback shapes.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// The SHIFT-WRITEBACK family (s_floor arm2's core): statements =
    /// `[i = C >> j0]  [if (test) return x]  [mutations...]  [stores...]`
    /// with a multi-use shifted mask. Registers come from the fitted
    /// int_alloc v2 model (13/13 captures — docs/int-allocator-frontier.md):
    /// a synthetic position pass numbers the template, values classify as
    /// Temp/Mask/Computed/Load{Discarded,Surviving}/Shift, and the model
    /// orders lowest-free assignment. Measured forms:
    ///   test: `((a & i) | b) == 0` (and + or., b FIRST) or `(a & i) == 0`
    ///     (and. record); skip = bne CONT; b EPI; CONT:.
    ///   mutations: `l &= ~i` (fused andc; TWO of them share one not r0),
    ///     `l &= K` (clrlwi r0, store from r0 — the home is read only),
    ///     `l = K` (li r0, store from r0 — the home is DISCARDED when it
    ///     was read in the test, and never loaded when read nowhere).
    pub(crate) fn try_punned_shift_writeback(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        if function.return_type != Type::Double
            || !function.guards.is_empty()
            || function_makes_call(function)
            || self.non_leaf
        {
            return Ok(false);
        }
        let Some(Expression::Variable(returned)) = &function.return_expression else {
            return Ok(false);
        };
        let Some(first) = function.parameters.first() else {
            return Ok(false);
        };
        if first.parameter_type != Type::Double || returned != &first.name {
            return Ok(false);
        }
        let x = first.name.as_str();
        // Roles come either from local INITIALIZERS (the normalizer folds
        // the leading assigns when nothing reassigns at top level — the
        // guarded-mutation forms) or from the LEADING assigns themselves
        // (top-level mutations make the normalizer refuse).
        let mut locals: Vec<(&str, i16)> = Vec::new();
        let mut guard: Option<GuardLocal> = None;
        let mut shift: Option<&str> = None;
        let mut mask_constant: Option<(i64, bool, i64)> = None; // (C, logical, amount offset)
        let mut cursor = 0usize;
        // The carry local (arm3's `j`) is assigned only inside the guard,
        // so the normalizer leaves it uninitialized while folding the rest.
        let mut carry_local: Option<&str> = None;
        let normalized = !function.locals.is_empty()
            && function.locals.iter().any(|local| local.initializer.is_some());
        if normalized {
            for local in &function.locals {
                if local.array_length.is_some() {
                    return Ok(false);
                }
                let Some(init) = local.initializer.as_ref() else {
                    if local.declared_type == Type::UnsignedInt && carry_local.is_none() {
                        carry_local = Some(local.name.as_str());
                        continue;
                    }
                    return Ok(false);
                };
                if local.declared_type == Type::UnsignedInt {
                    if shift.is_some() {
                        return Ok(false);
                    }
                    let Some(parsed) = &guard else { return Ok(false) };
                    let Some((constant, logical, offset)) = parse_shift_init(init, parsed.name)
                    else {
                        return Ok(false);
                    };
                    mask_constant = Some((constant, logical, offset));
                    shift = Some(local.name.as_str());
                    continue;
                }
                if local.declared_type != Type::Int {
                    return Ok(false);
                }
                if let Some(offset) = crate::frame::pun_word_offset_pub(init, x) {
                    if locals.iter().any(|&(_, seen)| seen == offset) {
                        return Ok(false);
                    }
                    locals.push((local.name.as_str(), offset));
                    continue;
                }
                if guard.is_some() {
                    return Ok(false);
                }
                let Some(parsed) = parse_guard_init(local.name.as_str(), init) else {
                    return Ok(false);
                };
                if !locals.iter().any(|&(name, _)| name == parsed.source) {
                    return Ok(false);
                }
                guard = Some(parsed);
            }
        } else {
            while let Some(Statement::Assign { name, value }) = function.statements.get(cursor) {
                let Some(declaration) = function.locals.iter().find(|local| &local.name == name) else {
                    return Ok(false);
                };
                if declaration.initializer.is_some() || declaration.array_length.is_some() {
                    return Ok(false);
                }
                if let Some(offset) = crate::frame::pun_word_offset_pub(value, x) {
                    if declaration.declared_type != Type::Int
                        || locals.iter().any(|&(_, seen)| seen == offset)
                    {
                        return Ok(false);
                    }
                    locals.push((name.as_str(), offset));
                    cursor += 1;
                    continue;
                }
                if guard.is_none() && declaration.declared_type == Type::Int {
                    if let Some(parsed) = parse_guard_init(name.as_str(), value) {
                        if locals.iter().any(|&(local, _)| local == parsed.source) {
                            guard = Some(parsed);
                            cursor += 1;
                            continue;
                        }
                    }
                    return Ok(false);
                }
                if shift.is_none() && declaration.declared_type == Type::UnsignedInt {
                    if let Some(parsed) = &guard {
                        if let Some((constant, logical, offset)) = parse_shift_init(value, parsed.name) {
                            mask_constant = Some((constant, logical, offset));
                            shift = Some(name.as_str());
                            cursor += 1;
                            continue;
                        }
                    }
                    return Ok(false);
                }
                return Ok(false);
            }
        }
        let (Some(guard), Some(shift), Some((mask_constant, logical_shift, amount_offset))) =
            (guard, shift, mask_constant)
        else {
            return Ok(false);
        };
        if i16::try_from(-amount_offset).is_err() {
            return Ok(false);
        }
        if locals.is_empty() || locals.len() > 2 {
            return Ok(false);
        }
        if guard.offset_k == 0 || i16::try_from(-guard.offset_k).is_err() {
            return Ok(false);
        }
        // j0 is consumed by the shift alone; the shift local is written once.
        let tail = &function.statements[cursor..];
        fn reads_in(statement: &Statement, name: &str) -> usize {
            match statement {
                Statement::Assign { value, .. } => count_name_occurrences(value, name),
                Statement::Store { target, value } => {
                    count_name_occurrences(target, name) + count_name_occurrences(value, name)
                }
                Statement::If { condition, then_body, else_body } => {
                    count_name_occurrences(condition, name)
                        + then_body.iter().map(|inner| reads_in(inner, name)).sum::<usize>()
                        + else_body.iter().map(|inner| reads_in(inner, name)).sum::<usize>()
                }
                Statement::Return(Some(value)) => count_name_occurrences(value, name),
                _ => 1,
            }
        }
        let guard_tail_reads: usize =
            tail.iter().map(|statement| reads_in(statement, guard.name)).sum();
        if guard_tail_reads > 2 {
            // Beyond the sign block's reads (the self-add's shift, or the
            // carry diamond's ==K4 + K3-j0 pair — validated structurally
            // below; unknown j0 uses fail the exact-form parses).
            return Ok(false);
        }
        // The early-return test.
        let Some(Statement::If { condition, then_body, else_body }) = tail.first() else {
            return Ok(false);
        };
        if !matches!(then_body.as_slice(), [Statement::Return(Some(Expression::Variable(v)))] if v == x)
            || !else_body.is_empty()
        {
            return Ok(false);
        }
        // `((a & i) | b) == 0` or `(a & i) == 0`, a/b punned, i the shift.
        let Expression::Binary { operator: BinaryOperator::Equal, left: test, right: zero } = condition
        else {
            return Ok(false);
        };
        if crate::analysis::constant_value(zero) != Some(0) {
            return Ok(false);
        }
        let local_index = |name: &str| locals.iter().position(|&(local, _)| local == name);
        let parse_and = |expr: &Expression| -> Option<usize> {
            let Expression::Binary { operator: BinaryOperator::BitAnd, left, right } = expr else {
                return None;
            };
            let Expression::Variable(a) = left.as_ref() else { return None };
            let Expression::Variable(i) = right.as_ref() else { return None };
            if i != shift {
                return None;
            }
            local_index(a)
        };
        let (test_and_local, test_or_local) = match test.as_ref() {
            Expression::Binary { operator: BinaryOperator::BitOr, left, right } => {
                let Some(a) = parse_and(left) else { return Ok(false) };
                let Expression::Variable(b) = right.as_ref() else { return Ok(false) };
                let Some(b) = local_index(b) else { return Ok(false) };
                (a, Some(b))
            }
            other => {
                let Some(a) = parse_and(other) else { return Ok(false) };
                (a, None)
            }
        };
        // An optional inexact guard wraps the mutations: `if (huge+x>0.0)
        // { [if (l<0) l += C2>>j0;] mutations }` (s_floor arm2). Inside
        // it a rewrite is CONDITIONAL — the original must survive the
        // guard-false path, so it lands in the home, not r0.
        let mut float_guard: Option<(u64, u64)> = None;
        enum SignBlock {
            Add { local: usize, constant: i64 },
            CarryDiamond {
                local: usize,          // i0 — takes +1
                other: usize,          // i1 — the carry source, receives j
                equal_bound: i16,      // j0 == K4
                shift_base: i16,       // K3 in `1 << (K3 - j0)`
            },
        }
        let mut sign_block: Option<SignBlock> = None;
        let mut mutation_statements: &[Statement] = &tail[1..];
        if let Some(Statement::If { condition, then_body, else_body }) = tail.get(1) {
            let Some(guard_bits) = float_guard_condition(condition) else {
                return Ok(false);
            };
            if !else_body.is_empty() {
                return Ok(false);
            }
            float_guard = Some(guard_bits);
            let mut body: &[Statement] = then_body;
            if let Some(Statement::If { condition, then_body: sign_body, else_body }) = body.first() {
                // `if (l < 0) ...`
                let Expression::Binary { operator: BinaryOperator::Less, left, right } = condition
                else {
                    return Ok(false);
                };
                if crate::analysis::constant_value(right) != Some(0) {
                    return Ok(false);
                }
                let Expression::Variable(signed) = left.as_ref() else { return Ok(false) };
                let Some(sign_local) = local_index(signed) else { return Ok(false) };
                if !else_body.is_empty() {
                    return Ok(false);
                }
                match sign_body.as_slice() {
                    // arm2: `l += C2 >> j0;`
                    [Statement::Assign { name: add_name, value: add_value }] => {
                        if local_index(add_name) != Some(sign_local) {
                            return Ok(false);
                        }
                        let Expression::Binary { operator: BinaryOperator::Add, left: base, right: shifted } =
                            add_value
                        else {
                            return Ok(false);
                        };
                        if !matches!(base.as_ref(), Expression::Variable(v) if v == add_name.as_str()) {
                            return Ok(false);
                        }
                        let Expression::Binary { operator: BinaryOperator::ShiftRight, left: c2, right: by } =
                            shifted.as_ref()
                        else {
                            return Ok(false);
                        };
                        let Some(c2) = crate::analysis::constant_value(c2) else { return Ok(false) };
                        if !matches!(by.as_ref(), Expression::Variable(v) if v == guard.name) {
                            return Ok(false);
                        }
                        sign_block = Some(SignBlock::Add { local: sign_local, constant: c2 });
                    }
                    // arm3: `if (j0 == K4) l += 1; else { j = other + (1 << (K3 - j0));
                    //        if (j < other) l += 1; other = j; }`
                    [Statement::If { condition, then_body, else_body }] => {
                        let Some(carry) = carry_local else { return Ok(false) };
                        let Expression::Binary { operator: BinaryOperator::Equal, left, right } = condition
                        else {
                            return Ok(false);
                        };
                        if !matches!(left.as_ref(), Expression::Variable(v) if v == guard.name) {
                            return Ok(false);
                        }
                        let Some(equal_bound) =
                            crate::analysis::constant_value(right).and_then(|k| i16::try_from(k).ok())
                        else {
                            return Ok(false);
                        };
                        // then: l += 1
                        let [Statement::Assign { name: inc, value: inc_value }] = then_body.as_slice()
                        else {
                            return Ok(false);
                        };
                        if local_index(inc) != Some(sign_local)
                            || !matches!(inc_value,
                                Expression::Binary { operator: BinaryOperator::Add, left, right }
                                    if matches!(left.as_ref(), Expression::Variable(v) if v == inc.as_str())
                                        && crate::analysis::constant_value(right) == Some(1))
                        {
                            return Ok(false);
                        }
                        // else: the carry sequence
                        let [Statement::Assign { name: j_name, value: j_value }, Statement::If { condition: carry_cond, then_body: carry_then, else_body: carry_else }, Statement::Assign { name: copy_name, value: copy_value }] =
                            else_body.as_slice()
                        else {
                            return Ok(false);
                        };
                        if j_name != carry {
                            return Ok(false);
                        }
                        let Expression::Binary { operator: BinaryOperator::Add, left: base, right: one_shift } =
                            j_value
                        else {
                            return Ok(false);
                        };
                        let Expression::Variable(other_name) = base.as_ref() else { return Ok(false) };
                        let Some(other) = local_index(other_name) else { return Ok(false) };
                        let Expression::Binary { operator: BinaryOperator::ShiftLeft, left: one, right: amount } =
                            one_shift.as_ref()
                        else {
                            return Ok(false);
                        };
                        if crate::analysis::constant_value(one) != Some(1) {
                            return Ok(false);
                        }
                        let Expression::Binary { operator: BinaryOperator::Subtract, left: k3, right: by } =
                            amount.as_ref()
                        else {
                            return Ok(false);
                        };
                        let Some(shift_base) =
                            crate::analysis::constant_value(k3).and_then(|k| i16::try_from(k).ok())
                        else {
                            return Ok(false);
                        };
                        if !matches!(by.as_ref(), Expression::Variable(v) if v == guard.name) {
                            return Ok(false);
                        }
                        // if (j < other) l += 1;
                        if !carry_else.is_empty() {
                            return Ok(false);
                        }
                        let Expression::Binary { operator: BinaryOperator::Less, left: jl, right: jr } =
                            carry_cond
                        else {
                            return Ok(false);
                        };
                        if !matches!(jl.as_ref(), Expression::Variable(v) if v == carry)
                            || !matches!(jr.as_ref(), Expression::Variable(v) if local_index(v) == Some(other))
                        {
                            return Ok(false);
                        }
                        let [Statement::Assign { name: inc2, value: inc2_value }] = carry_then.as_slice()
                        else {
                            return Ok(false);
                        };
                        if local_index(inc2) != Some(sign_local)
                            || !matches!(inc2_value,
                                Expression::Binary { operator: BinaryOperator::Add, left, right }
                                    if matches!(left.as_ref(), Expression::Variable(v) if v == inc2.as_str())
                                        && crate::analysis::constant_value(right) == Some(1))
                        {
                            return Ok(false);
                        }
                        // other = j
                        if local_index(copy_name) != Some(other)
                            || !matches!(copy_value, Expression::Variable(v) if v == carry)
                        {
                            return Ok(false);
                        }
                        sign_block = Some(SignBlock::CarryDiamond {
                            local: sign_local,
                            other,
                            equal_bound,
                            shift_base,
                        });
                    }
                    _ => return Ok(false),
                }
                body = &body[1..];
            }
            mutation_statements = body;
        }
        // The self-add's constant must equal the mask synthesis' lis
        // intermediate — mwcc reuses the materialized register (measured:
        // 0x00100000 for the 0xfffff mask). Anything else is unprobed.
        // The constant wraps to its 32-bit value (0xffffffff = li -1).
        let mask_constant = mask_constant as u32 as i32 as i64;
        let needs_temp_early = i16::try_from(mask_constant).is_err();
        let lis_intermediate = ((mask_constant + 0x8000) >> 16) << 16;
        if let Some(SignBlock::Add { constant, .. }) = sign_block {
            // The self-add's constant must CSE the lis intermediate.
            if !needs_temp_early || constant != lis_intermediate || float_guard.is_none() {
                return Ok(false);
            }
        }
        if matches!(sign_block, Some(SignBlock::CarryDiamond { .. })) && float_guard.is_none() {
            return Ok(false);
        }
        if carry_local.is_some() && !matches!(sign_block, Some(SignBlock::CarryDiamond { .. })) {
            return Ok(false);
        }
        // j0's reads beyond the mask shift: the self-add's shift, or the
        // carry diamond's ==K4 and K3-j0.
        let guard_multi_read = sign_block.is_some() || amount_offset != 0;
        // Mutations, then stores.
        enum Mutation {
            Rewrite(i16),
            AndcShift,
            MaskViaScratch { begin: u8, end: u8 },
        }
        let mut mutations: Vec<(usize, Mutation)> = Vec::new();
        let mut tail_cursor = 0usize;
        while let Some(Statement::Assign { name, value }) = mutation_statements.get(tail_cursor) {
            let Some(index) = local_index(name) else { return Ok(false) };
            if mutations.iter().any(|&(seen, _)| seen == index) {
                return Ok(false);
            }
            let mutation = if let Some(constant) = crate::analysis::constant_value(value) {
                let Ok(small) = i16::try_from(constant) else { return Ok(false) };
                Mutation::Rewrite(small)
            } else if let Expression::Binary { operator: BinaryOperator::BitAnd, left, right } = value {
                if !matches!(left.as_ref(), Expression::Variable(v) if v == name.as_str()) {
                    return Ok(false);
                }
                match right.as_ref() {
                    Expression::Unary { operator: UnaryOperator::BitNot, operand }
                        if matches!(operand.as_ref(), Expression::Variable(v) if v == shift) =>
                    {
                        Mutation::AndcShift
                    }
                    other => {
                        let Some((begin, end)) = crate::analysis::constant_value(other)
                            .and_then(crate::analysis::rlwinm_mask)
                        else {
                            return Ok(false);
                        };
                        if float_guard.is_some() {
                            // Unprobed inside a guard (the r0 handoff to the
                            // store would cross the guard bounds).
                            return Ok(false);
                        }
                        Mutation::MaskViaScratch { begin, end }
                    }
                }
            } else {
                return Ok(false);
            };
            mutations.push((index, mutation));
            tail_cursor += 1;
        }
        // At most one rewrite (the li r0 dedupe across two is unmeasured).
        if mutations.iter().filter(|(_, m)| matches!(m, Mutation::Rewrite(_))).count() > 1 {
            return Ok(false);
        }
        // The r0 materialization sinks below the home-writing mutations
        // regardless of source order (measured D3: andc; li r0; stores) —
        // r0's range stays minimal. A CONDITIONAL rewrite (inside the
        // guard) stays in source position: it writes the home.
        if float_guard.is_none() {
            mutations.sort_by_key(|(_, mutation)| matches!(mutation, Mutation::Rewrite(_)));
        }
        // Stores: one per local, its own offset, in local order. With a
        // guard the mutations exhaust its body and the stores follow the
        // guard-If in the outer tail.
        let stores = if float_guard.is_some() {
            if tail_cursor != mutation_statements.len() {
                return Ok(false);
            }
            &tail[2..]
        } else {
            &mutation_statements[tail_cursor..]
        };
        if stores.len() != locals.len() {
            return Ok(false);
        }
        for (statement, &(name, offset)) in stores.iter().zip(&locals) {
            let Statement::Store { target, value } = statement else {
                return Ok(false);
            };
            if crate::frame::pun_word_offset_pub(target, x) != Some(offset)
                || !matches!(value, Expression::Variable(read) if read == name)
            {
                return Ok(false);
            }
        }
        // -- the synthetic position pass --
        use mwcc_vreg::int_alloc::{allocate, Class, Value};
        let needs_temp = i16::try_from(mask_constant).is_err();
        let mut position = 1u32; // 0 = stwu
        let temp_range = needs_temp.then(|| {
            let range = (position, position + 1);
            position += 1;
            range
        });
        let mask_position = position; // li or the addi completing the pair
        position += 1;
        position += 1; // stfd
        // Which locals load: any with a read (test, extract source, andc/mask mutation).
        let has_read = |index: usize| {
            index == test_and_local
                || test_or_local == Some(index)
                || locals[index].0 == guard.source
                || matches!(sign_block, Some(SignBlock::Add { local, .. }) if local == index)
                || matches!(sign_block, Some(SignBlock::CarryDiamond { local, other, .. }) if local == index || other == index)
                || mutations.iter().any(|&(m, ref form)| {
                    m == index
                        && (!matches!(form, Mutation::Rewrite(_)) || float_guard.is_some())
                })
        };
        let mut load_positions: Vec<Option<u32>> = Vec::new();
        for index in 0..locals.len() {
            if has_read(index) {
                load_positions.push(Some(position));
                position += 1;
            } else {
                load_positions.push(None);
            }
        }
        let extract_position = position;
        position += 1;
        let fold_position = position;
        position += 1;
        let sraw_position = position;
        position += 1;
        let and_position = position;
        position += 1;
        let or_position = test_or_local.map(|_| {
            let at = position;
            position += 1;
            at
        });
        let branch_position = position; // bne
        position += 2; // bne + b
        // The inexact-guard block (lfd, lfd, fadd, fcmpo, ble) and the
        // sign-add (cmpwi, bge, sraw2, add).
        if float_guard.is_some() {
            position += 5;
        }
        // The sign block: Add = cmpwi, bge, sraw2, add; CarryDiamond =
        // cmpwi, bge, cmpwi, bne, addi, b, subfic, li, slw, add, cmplw,
        // bge, addi, mr.
        let mut carry_one_range: Option<(u32, u32)> = None;
        let sraw2_position = match &sign_block {
            Some(SignBlock::Add { .. }) => {
                position += 2; // cmpwi + bge
                let at = position;
                position += 2; // sraw2 + add
                Some(at)
            }
            Some(SignBlock::CarryDiamond { .. }) => {
                position += 6; // cmpwi, bge, cmpwi(==K4), bne, addi, b
                let subfic_at = position;
                position += 1;
                let one_at = position;
                position += 1; // li 1
                carry_one_range = Some((one_at, position)); // li..slw
                position += 6; // slw, add, cmplw, bge, addi, mr
                Some(subfic_at) // j0's last read = the subfic
            }
            None => None,
        };
        // Mutations occupy sequential slots (the shared `not` adds one).
        let andc_count = mutations.iter().filter(|(_, m)| matches!(m, Mutation::AndcShift)).count();
        let not_position = (andc_count >= 2).then(|| {
            let at = position;
            position += 1;
            at
        });
        let mut mutation_positions: Vec<u32> = Vec::new();
        for _ in &mutations {
            mutation_positions.push(position);
            position += 1;
        }
        let mut store_positions: Vec<u32> = Vec::new();
        for _ in &locals {
            store_positions.push(position);
            position += 1;
        }
        // -- classify + model --
        let mut values: Vec<Value> = Vec::new();
        let mut tags: Vec<&str> = Vec::new(); // parallel debug tags
        if let Some((lis, addi)) = temp_range {
            // The self-add's constant CSEs the lis intermediate — the
            // temp then lives to the second sraw (measured arm2).
            let last = sraw2_position.unwrap_or(addi);
            values.push(Value { class: Class::Temp, def: lis, last });
            tags.push("temp");
        }
        // With a MULTI-READ guard the fold lands in the home, freeing the
        // r0 timeline — the branch-free mask takes r0 itself (measured
        // arm2: addi r0,r3,-1).
        // ...unless an amount offset (arm3's j0-20) writes r0 inside the
        // mask's live range.
        let mask_in_scratch = guard_multi_read && amount_offset == 0;
        let mask_value_index = if mask_in_scratch {
            None
        } else {
            values.push(Value { class: Class::Mask, def: mask_position, last: sraw_position });
            tags.push("mask");
            Some(values.len() - 1)
        };
        let computed_last = sraw2_position.unwrap_or(fold_position);
        values.push(Value { class: Class::Computed, def: extract_position, last: computed_last });
        tags.push("computed");
        let computed_value_index = values.len() - 1;
        let carry_one_value_index = carry_one_range.map(|(def, last)| {
            values.push(Value { class: Class::Mask, def, last });
            tags.push("carry-one");
            values.len() - 1
        });
        // The shift local: last read = latest of the test and-op and any
        // andc/not mutation.
        let shift_last = if let Some(not_at) = not_position {
            not_at
        } else if let Some(at) = mutations
            .iter()
            .zip(&mutation_positions)
            .filter(|((_, m), _)| matches!(m, Mutation::AndcShift))
            .map(|(_, &at)| at)
            .max()
        {
            at
        } else {
            and_position
        };
        let shift_crosses = shift_last > branch_position;
        let shift_value_index = if shift_crosses {
            values.push(Value { class: Class::Shift, def: sraw_position, last: shift_last });
            tags.push("shift");
            Some(values.len() - 1)
        } else {
            None // r0 (branch-free single use)
        };
        let mut local_value_indices: Vec<Option<usize>> = vec![None; locals.len()];
        for index in 0..locals.len() {
            let Some(load) = load_positions[index] else { continue };
            // The home's last read.
            let mut last = load;
            if locals[index].0 == guard.source {
                last = last.max(extract_position);
            }
            if index == test_and_local {
                last = last.max(and_position);
            }
            if test_or_local == Some(index) {
                last = last.max(or_position.unwrap_or(and_position));
            }
            match &sign_block {
                Some(SignBlock::Add { local, .. }) if *local == index => {
                    // cmpwi + the add read/write the home inside the guard.
                    last = last.max(sraw2_position.expect("sign add") + 1);
                }
                Some(SignBlock::CarryDiamond { local, other, .. })
                    if *local == index || *other == index =>
                {
                    // The homes live through the whole diamond (the mr /
                    // the final addi).
                    last = last.max(sraw2_position.expect("carry") + 7);
                }
                _ => {}
            }
            let mutation = mutations
                .iter()
                .zip(&mutation_positions)
                .find(|((m, _), _)| *m == index);
            let class = match mutation {
                Some(((_, Mutation::Rewrite(_)), _)) if float_guard.is_none() => {
                    // The home dies at its last pre-branch read.
                    Class::LoadDiscarded
                }
                Some(((_, Mutation::Rewrite(_)), _)) => {
                    // A rewrite INSIDE the guard is conditional: the
                    // original flows to the store on the guard-false path.
                    last = last.max(store_positions[index]);
                    Class::LoadSurviving
                }
                Some(((_, Mutation::AndcShift), &at)) => {
                    // andc writes the home; the store reads it.
                    last = last.max(store_positions[index]);
                    let _ = at;
                    Class::LoadSurviving
                }
                Some(((_, Mutation::MaskViaScratch { .. }), &at)) => {
                    // clrlwi reads the home; the store reads r0.
                    last = last.max(at);
                    Class::LoadSurviving
                }
                None => {
                    last = last.max(store_positions[index]);
                    Class::LoadSurviving
                }
            };
            values.push(Value { class, def: load, last });
            tags.push("local");
            local_value_indices[index] = Some(values.len() - 1);
        }
        let registers = allocate(&values);
        let _ = &tags;
        let mask_register = mask_value_index.map(|i| registers[i]).unwrap_or(0);
        let guard_register = registers[computed_value_index];
        let shift_register = shift_value_index.map(|i| registers[i]).unwrap_or(0);
        let home = |index: usize| local_value_indices[index].map(|i| registers[i]);
        // -- emit --
        self.frame_size = 16;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        if needs_temp {
            let temp_register = registers[0];
            let high = ((mask_constant + 0x8000) >> 16) as i16;
            let low = mask_constant as i16;
            self.output.instructions.push(Instruction::load_immediate_shifted(temp_register, high));
            self.output.instructions.push(Instruction::AddImmediate {
                d: mask_register,
                a: temp_register,
                immediate: low,
            });
        } else {
            self.output.instructions.push(Instruction::load_immediate(mask_register, mask_constant as i16));
        }
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 8 });
        for (index, &(_, offset)) in locals.iter().enumerate() {
            if load_positions[index].is_some() {
                self.output.instructions.push(Instruction::LoadWord {
                    d: home(index).expect("loaded"),
                    a: 1,
                    offset: 8 + offset,
                });
            }
        }
        let source_home = home(local_index(guard.source).expect("validated")).expect("source loads");
        match guard.mask {
            Some(mask) => {
                let rotated = ((32 - guard.shift as u32) % 32) as u8;
                let Some((begin, end)) = crate::analysis::rlwinm_mask(mask) else {
                    return Err(Diagnostic::error("guard mask is not a run (roadmap)"));
                };
                self.output.instructions.push(Instruction::RotateAndMask {
                    a: guard_register,
                    s: source_home,
                    shift: rotated,
                    begin,
                    end,
                });
            }
            None => {
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate {
                    a: guard_register,
                    s: source_home,
                    shift: guard.shift,
                });
            }
        }
        let negative = i16::try_from(-guard.offset_k).expect("validated");
        let shift_amount = if guard_multi_read {
            // Multiple j0 reads: the -K lands in the home; an amount
            // offset (arm3's j0-20) folds separately into r0.
            self.output.instructions.push(Instruction::AddImmediate {
                d: guard_register,
                a: guard_register,
                immediate: negative,
            });
            if amount_offset != 0 {
                self.output.instructions.push(Instruction::AddImmediate {
                    d: 0,
                    a: guard_register,
                    immediate: i16::try_from(-amount_offset).expect("validated"),
                });
                0
            } else {
                guard_register
            }
        } else {
            self.output.instructions.push(Instruction::AddImmediate { d: 0, a: guard_register, immediate: negative });
            0
        };
        if logical_shift {
            self.output.instructions.push(Instruction::ShiftRightWord {
                a: shift_register,
                s: mask_register,
                b: shift_amount,
            });
        } else {
            self.output.instructions.push(Instruction::ShiftRightAlgebraicWord {
                a: shift_register,
                s: mask_register,
                b: shift_amount,
            });
        }
        // The test.
        let and_home = home(test_and_local).expect("test local loads");
        if let Some(or_local) = test_or_local {
            self.output.instructions.push(Instruction::And { a: 0, s: and_home, b: shift_register });
            self.output.instructions.push(Instruction::OrRecord {
                a: 0,
                s: home(or_local).expect("test local loads"),
                b: 0,
            });
        } else {
            self.output.instructions.push(Instruction::AndRecord { a: 0, s: and_home, b: shift_register });
        }
        let continuation = self.fresh_label();
        let epilogue = self.fresh_label();
        let join = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, continuation); // bne — skip the return
        self.emit_branch_to(epilogue);
        self.bind_label(continuation);
        if let Some((huge, zero)) = float_guard {
            // The nested inexact guard (the G2 recipe): huge/0.0 pool-load
            // back-to-back into f2/f0, fadd clobbers the spilled f1, ble
            // chains to the join.
            self.load_double_constant(2, huge);
            self.load_double_constant(0, zero);
            self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 2, b: 1 });
            self.output.instructions.push(Instruction::FloatCompareOrdered { a: 1, b: 0 });
            self.emit_branch_conditional_to(4, 1, join);
        }
        match &sign_block {
            Some(SignBlock::Add { local, .. }) => {
                // `if (l < 0) l += C2 >> j0` — C2 reuses the lis intermediate.
                let register = home(*local).expect("sign local loads");
                let temp_register = registers[0];
                let skip = self.fresh_label();
                self.output.instructions.push(Instruction::CompareWordImmediate { a: register, immediate: 0 });
                self.emit_branch_conditional_to(4, 0, skip); // bge
                self.output.instructions.push(Instruction::ShiftRightAlgebraicWord {
                    a: 0,
                    s: temp_register,
                    b: guard_register,
                });
                self.output.instructions.push(Instruction::Add { d: register, a: register, b: 0 });
                self.bind_label(skip);
            }
            Some(SignBlock::CarryDiamond { local, other, equal_bound, shift_base }) => {
                // `if (l < 0) { if (j0 == K4) l += 1; else { j = other +
                // (1 << (K3 - j0)); if (j < other) l += 1; other = j; } }`
                // — j lives in r0; the ONE constant takes a model register
                // (arm3: the dead mask's r3).
                let register = home(*local).expect("sign local loads");
                let other_register = home(*other).expect("carry source loads");
                let one_register = carry_one_value_index
                    .map(|i| registers[i])
                    .expect("carry one allocated");
                let continue_at = self.fresh_label(); // the trailing mutations
                let else_at = self.fresh_label();
                let no_carry = self.fresh_label();
                self.output.instructions.push(Instruction::CompareWordImmediate { a: register, immediate: 0 });
                self.emit_branch_conditional_to(4, 0, continue_at); // bge — skip the diamond
                self.output.instructions.push(Instruction::CompareWordImmediate {
                    a: guard_register,
                    immediate: *equal_bound,
                });
                self.emit_branch_conditional_to(4, 2, else_at); // bne
                self.output.instructions.push(Instruction::AddImmediate { d: register, a: register, immediate: 1 });
                self.emit_branch_to(continue_at);
                self.bind_label(else_at);
                self.output.instructions.push(Instruction::SubtractFromImmediate {
                    d: 0,
                    a: guard_register,
                    immediate: *shift_base,
                });
                self.output.instructions.push(Instruction::load_immediate(one_register, 1));
                self.output.instructions.push(Instruction::ShiftLeftWord { a: 0, s: one_register, b: 0 });
                self.output.instructions.push(Instruction::Add { d: 0, a: other_register, b: 0 });
                self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: other_register });
                self.emit_branch_conditional_to(4, 0, no_carry); // bge — unsigned no-carry
                self.output.instructions.push(Instruction::AddImmediate { d: register, a: register, immediate: 1 });
                self.bind_label(no_carry);
                self.output.instructions.push(Instruction::move_register(other_register, 0));
                self.bind_label(continue_at);
            }
            None => {}
        }
        // Mutations (the shared `not` precedes the first andc pair).
        if not_position.is_some() {
            self.output.instructions.push(Instruction::Nor { a: 0, s: shift_register, b: shift_register });
        }
        for (index, mutation) in &mutations {
            let index = *index;
            match mutation {
                Mutation::Rewrite(constant) => {
                    // Conditional (guarded) rewrites write the HOME — the
                    // original flows to the store on the guard-false path.
                    let target = if float_guard.is_some() {
                        home(index).expect("conditional rewrite loads")
                    } else {
                        0
                    };
                    self.output.instructions.push(Instruction::load_immediate(target, *constant));
                }
                Mutation::AndcShift => {
                    let register = home(index).expect("loaded");
                    if not_position.is_some() {
                        self.output.instructions.push(Instruction::And { a: register, s: register, b: 0 });
                    } else {
                        self.output.instructions.push(Instruction::AndComplement {
                            a: register,
                            s: register,
                            b: shift_register,
                        });
                    }
                }
                Mutation::MaskViaScratch { begin, end } => {
                    self.output.instructions.push(Instruction::RotateAndMask {
                        a: 0,
                        s: home(index).expect("loaded"),
                        shift: 0,
                        begin: *begin,
                        end: *end,
                    });
                }
            }
        }
        // Stores (the guard's ble lands here): surviving homes store
        // themselves; UNCONDITIONAL rewrites and mask-via-scratch store
        // from r0.
        self.bind_label(join);
        for (index, &(_, offset)) in locals.iter().enumerate() {
            let from_scratch = float_guard.is_none()
                && mutations.iter().any(|&(m, ref form)| {
                    m == index && !matches!(form, Mutation::AndcShift)
                });
            let register = if from_scratch { 0 } else { home(index).map(|r| r).unwrap_or(0) };
            self.output.instructions.push(Instruction::StoreWord { s: register, a: 1, offset: 8 + offset });
        }
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 8 });
        self.bind_label(epilogue);
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // Pre-pool labels: one plus one per LOADED local (measured V1c
        // @7 and W11 @7 with one load, V1b @8 with two — the never-read
        // store-only local costs nothing), plus one for the shared `not`
        // temp (W10 @9).
        self.output.anonymous_label_bump += 1
            + load_positions.iter().filter(|p| p.is_some()).count() as u32
            + not_position.is_some() as u32
            + 2 * float_guard.is_some() as u32
            + match &sign_block {
                Some(SignBlock::Add { .. }) => 2,
                // Three inner conditions (sign, ==K4, the carry compare)
                // at two each, one else arm, one for the ONE temp
                // (measured @18 on the arm3 object).
                Some(SignBlock::CarryDiamond { .. }) => 8,
                None => 0,
            };
        Ok(true)
    }

}
