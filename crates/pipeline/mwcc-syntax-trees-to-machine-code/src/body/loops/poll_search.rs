//! Polling and search loops (busy-wait, list-search, norm, flag-while).
//!
//! Split from a single 2795-line `loops.rs` (behavior-identical).

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// A leaf `void` function whose whole body is an EMPTY-body busy-wait on one
    /// fixed-address array element — the hardware-register poll (`while (__EXIRegs[13] & 1);`,
    /// DebuggerDriver/EXI/SI/DSP spin loops). mwcc materializes the ELEMENT address once
    /// (`lis`/`addi` of the folded `base + index*elem`), then loops load → test → branch
    /// back (the volatile reload is the loop):
    ///
    /// ```text
    ///   lis rB, elem@ha ; addi rB, rB, elem@lo
    ///   loop: lwz r0,0(rB) ; rlwinm. r0,r0,0,mb,me ; bne loop     (`& CONTIGUOUS_MASK`)
    ///                        cmplwi r0,0            ; bne loop     (truthy)
    /// ```
    ///
    /// A `!(…)` wrapper flips `bne` to `beq` (wait-until-set). Element widths: u32 `lwz`,
    /// u16 `lhz`, u8 `lbz`. Measured 1.3.2/2.0/2.7: masks 1/3/0x100/0x8000/0x200-on-u16,
    /// truthy, negated. A non-contiguous mask, non-constant index, or any other condition
    /// shape falls through (the general loop defer). Returns whether this path applied.
    pub(crate) fn try_emit_busy_wait(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type != Type::Void
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        let [Statement::Loop {
            kind,
            initializer: None,
            condition: Some(condition),
            step: None,
            body,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if !matches!(kind, LoopKind::While | LoopKind::DoWhile) {
            return Ok(false);
        }
        if !body.is_empty() {
            return Ok(false);
        }
        // Strip a logical-not wrapper: `while (!(R[i] & m));` waits for the bit to SET,
        // so the backward branch re-enters while the test result is ZERO (`beq`).
        let (condition, negated) = match condition {
            Expression::Unary {
                operator: UnaryOperator::LogicalNot,
                operand,
            } => (operand.as_ref(), true),
            other => (other, false),
        };
        // The testable forms: `R[c] & mask` (contiguous mask -> one `rlwinm.`) or bare `R[c]`.
        let (element_access, mask) = match condition {
            Expression::Binary {
                operator: BinaryOperator::BitAnd,
                left,
                right,
            } => match (left.as_ref(), right.as_ref()) {
                (access, Expression::IntegerLiteral(mask)) => (access, Some(*mask)),
                (Expression::IntegerLiteral(mask), access) => (access, Some(*mask)),
                _ => return Ok(false),
            },
            access => (access, None),
        };
        let Expression::Index { base, index } = element_access else {
            return Ok(false);
        };
        let (Expression::Variable(name), Expression::IntegerLiteral(index)) =
            (base.as_ref(), index.as_ref())
        else {
            return Ok(false);
        };
        let Some(&(address, element)) = self.fixed_address_arrays.get(name) else {
            return Ok(false);
        };
        // The mask must be one contiguous run of ones (a single `rlwinm.` range).
        let mask_bits = match mask {
            Some(mask) => {
                let bits = mask as u64 as u32;
                if bits == 0 {
                    return Ok(false);
                }
                let low = bits.trailing_zeros();
                let high = 31 - bits.leading_zeros();
                let contiguous = (bits >> low).count_ones() == high - low + 1
                    && bits >> low == (1u64 << (high - low + 1)) as u32 - 1;
                if !contiguous {
                    return Ok(false);
                }
                Some((31 - high) as u8..=(31 - low) as u8) // PPC bit numbering: mb..=me
            }
            None => None,
        };
        let (load, element_bytes): (fn(u8, u8, i16) -> Instruction, u32) = match element {
            Type::Int | Type::UnsignedInt => {
                (|d, a, offset| Instruction::LoadWord { d, a, offset }, 4)
            }
            Type::Short | Type::UnsignedShort => (
                |d, a, offset| Instruction::LoadHalfwordZero { d, a, offset },
                2,
            ),
            Type::Char | Type::UnsignedChar => {
                (|d, a, offset| Instruction::LoadByteZero { d, a, offset }, 1)
            }
            _ => return Ok(false),
        };

        // The loop-invariant address: element 0's hoisted invariant is just the `lis`
        // half (the `lo` rides the load's displacement, re-read each iteration);
        // a non-zero element hoists the FULL folded address (`lis`+`addi`) and the
        // load runs at displacement 0. Measured: R[0] -> `lis; loop: lwz lo(rB)`,
        // R[13] -> `lis; addi; loop: lwz 0(rB)`.
        let element_address = address as u32 + *index as u32 * element_bytes;
        let base_register = self.lowest_free_general()?;
        let high = ((element_address.wrapping_add(0x8000)) >> 16) as u16;
        let low = element_address as u16 as i16;
        // The loop's internal labels advance mwcc's anonymous-`@N` counter: by 6 for
        // an element-0 poll, by 7 for a non-zero element (the folded full-address
        // temporary adds one) — measured against the no-loop baseline (@9 -> @15/@16).
        self.output.anonymous_label_bump = if *index == 0 { 6 } else { 7 };
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: base_register,
                a: 0,
                immediate: high as i16,
            });
        let load_offset = if *index == 0 {
            low
        } else {
            self.output.instructions.push(Instruction::AddImmediate {
                d: base_register,
                a: base_register,
                immediate: low,
            });
            0
        };

        // loop: load; test (sets cr0); branch back while waiting.
        let loop_top = self.output.instructions.len();
        self.output
            .instructions
            .push(load(0, base_register, load_offset));
        match mask_bits {
            Some(range) => self
                .output
                .instructions
                .push(Instruction::RotateAndMaskRecord {
                    a: 0,
                    s: 0,
                    shift: 0,
                    begin: *range.start(),
                    end: *range.end(),
                }),
            None => self
                .output
                .instructions
                .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 }),
        }
        // `bne loop` re-enters while the bit is SET (wait-for-clear); a negated
        // condition re-enters while ZERO (`beq loop`, wait-for-set).
        let (options, condition_bit) = if negated { (12, 2) } else { (4, 2) };
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options,
                condition_bit,
                target: loop_top,
            });
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// `T* f(T* p, …) { while (p) { if (p->field CMP x) return p; p = p->next; } return 0; }`
    /// — a linked-list search. mwcc keeps the rotated chase loop and lowers the in-body
    /// early return to a `bclr` (the searched pointer is already in r3, returned unmoved),
    /// followed by the null default after the loop. Leaf; gated to the exact search shape.
    pub(crate) fn try_list_search_loop(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::LoopKind;
        if !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || !function.locals.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        if matches!(function.return_type, Type::Float | Type::Double)
            || function.return_type == Type::Void
        {
            return Ok(false);
        }
        let [Statement::Loop {
            kind,
            initializer: None,
            condition: Some(condition),
            step: None,
            body,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if !matches!(kind, LoopKind::While | LoopKind::DoWhile) {
            return Ok(false);
        }
        // A constant default return after the loop (`return 0;`).
        let Some(default_return) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        if constant_value(default_return).is_none() {
            return Ok(false);
        }
        // `while (p)` — the searched pointer, which must be the FIRST parameter so it sits
        // in r3 and the in-body `return p` is a bare `bclr` (no move).
        let Expression::Variable(loop_ptr) = condition else {
            return Ok(false);
        };
        if function.parameters.first().map(|parameter| &parameter.name) != Some(loop_ptr) {
            return Ok(false);
        }
        let Some(loop_register) = self.lookup_general(loop_ptr) else {
            return Ok(false);
        };
        if loop_register != Eabi::general_result().number {
            return Ok(false);
        }
        // Body = [ if (COND) return <p>; , <p> = <chase of p>; ] with an empty else.
        let [Statement::If {
            condition: if_condition,
            then_body,
            else_body,
        }, Statement::Assign {
            name: chase_name,
            value: chase_value,
        }] = body.as_slice()
        else {
            return Ok(false);
        };
        if !else_body.is_empty() {
            return Ok(false);
        }
        // The in-body early return: either the loop pointer itself (a bare `bclr`, no
        // move) or a constant flag (materialize + `blr`, reached past a forward branch
        // that skips the found arm when the condition is false).
        let [Statement::Return(Some(return_value))] = then_body.as_slice() else {
            return Ok(false);
        };
        let returns_pointer =
            matches!(return_value, Expression::Variable(other) if other == loop_ptr);
        if (!returns_pointer && constant_value(return_value).is_none()) || chase_name != loop_ptr {
            return Ok(false);
        }
        let is_chase = matches!(chase_value, Expression::Member { base, .. }
                if matches!(base.as_ref(), Expression::Variable(other) if other == loop_ptr))
            || matches!(chase_value, Expression::Dereference { pointer }
                if matches!(pointer.as_ref(), Expression::Variable(other) if other == loop_ptr));
        if !is_chase {
            return Ok(false);
        }

        // -- emit: b test; body{ if-cond, found-arm, chase }; test: cmplwi; bne body; default; blr --
        self.output.anonymous_label_bump = 6; // while (4) + the inner if (2)
        let result = Eabi::general_result().number;
        let skip = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });
        let body_top = self.output.instructions.len();
        let (skip_options, if_bit) = self.emit_condition_test(if_condition)?;
        if returns_pointer {
            // Return the searched pointer (already in r3) via `bclr` when TRUE — invert
            // emit_condition_test's SKIP branch; the chase falls through after.
            let return_options = if skip_options == 4 { 12 } else { 4 };
            self.output
                .instructions
                .push(Instruction::BranchConditionalToLinkRegister {
                    options: return_options,
                    condition_bit: if_bit,
                });
            self.evaluate_general(chase_value, loop_register)?;
        } else {
            // Skip the found arm to the chase when FALSE (the emit_condition_test SKIP
            // branch used directly), else materialize the flag and return.
            let to_chase = self.output.instructions.len();
            self.output
                .instructions
                .push(Instruction::BranchConditionalForward {
                    options: skip_options,
                    condition_bit: if_bit,
                    target: 0,
                });
            self.evaluate_tail(return_value, function.return_type, result)?;
            self.output
                .instructions
                .push(Instruction::BranchToLinkRegister);
            let chase_at = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } =
                &mut self.output.instructions[to_chase]
            {
                *target = chase_at;
            }
            self.evaluate_general(chase_value, loop_register)?;
        }
        let condition_at = self.output.instructions.len();
        if let Instruction::Branch { target } = &mut self.output.instructions[skip] {
            *target = condition_at;
        }
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let back = if options == 4 { 12 } else { 4 };
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: back,
                condition_bit,
                target: body_top,
            });
        self.evaluate_tail(default_return, function.return_type, result)?;
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// The NORMALIZE LOOP (fire 424, e_fmod's tail loop): a NON-counted
    /// `while (hx < BIG) { hx = hx+hx+(lx>>31); lx = lx+lx; iy -= 1; }`
    /// with `return hx + iy` — rotated form with the big bound hoisted
    /// `lis r0, BIG>>16` BEFORE the loop. r0 stays OCCUPIED across the
    /// body, so the carry temp takes the next free register after the
    /// params; the iy decrement schedules INTO the add latency (between
    /// `add rT,hx,rT` and `add hx,hx,rT`). Bound gated to lis-only
    /// constants (low half zero, not a cmpwi immediate).
    pub(crate) fn try_norm_loop(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::{LoopKind, Statement};
        if function.return_type != Type::Int
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
            || !function.locals.is_empty()
        {
            return Ok(false);
        }
        let [p_hx, p_lx, p_iy] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if p_hx.parameter_type != Type::Int
            || p_lx.parameter_type != Type::UnsignedInt
            || p_iy.parameter_type != Type::Int
        {
            return Ok(false);
        }
        let (hx, lx, iy) = (p_hx.name.as_str(), p_lx.name.as_str(), p_iy.name.as_str());
        if hx == lx || hx == iy || lx == iy {
            return Ok(false);
        }
        let [Statement::Loop {
            kind,
            initializer: None,
            condition: Some(condition),
            step: None,
            body,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if !matches!(kind, LoopKind::While | LoopKind::DoWhile) {
            return Ok(false);
        }
        // The condition: hx < BIG, BIG a lis-only constant (low half 0).
        let Expression::Binary {
            operator: BinaryOperator::Less,
            left: test_left,
            right: test_right,
        } = condition
        else {
            return Ok(false);
        };
        if !matches!(test_left.as_ref(), Expression::Variable(v) if v == hx) {
            return Ok(false);
        }
        let Expression::IntegerLiteral(bound) = test_right.as_ref() else {
            return Ok(false);
        };
        let bound = *bound;
        if bound & 0xffff != 0 {
            return Ok(false);
        }
        let Ok(bound_high) = i16::try_from(bound >> 16) else {
            return Ok(false);
        };
        // The body: [hx = hx+hx+(lx>>31)][lx = lx+lx][iy = iy-1].
        let [Statement::Assign {
            name: high_name,
            value: high_value,
        }, Statement::Assign {
            name: low_name,
            value: low_value,
        }, Statement::Assign {
            name: dec_name,
            value: dec_value,
        }] = body.as_slice()
        else {
            return Ok(false);
        };
        if high_name != hx || low_name != lx || dec_name != iy {
            return Ok(false);
        }
        let Expression::Binary {
            operator: BinaryOperator::Add,
            left: sum,
            right: carry,
        } = high_value
        else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::Add,
            left: first,
            right: second,
        } = sum.as_ref()
        else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::ShiftRight,
            left: shifted,
            right: amount,
        } = carry.as_ref()
        else {
            return Ok(false);
        };
        if !matches!(first.as_ref(), Expression::Variable(v) if v == hx)
            || !matches!(second.as_ref(), Expression::Variable(v) if v == hx)
            || !matches!(shifted.as_ref(), Expression::Variable(v) if v == lx)
            || !matches!(amount.as_ref(), Expression::IntegerLiteral(31))
        {
            return Ok(false);
        }
        let Expression::Binary {
            operator: BinaryOperator::Add,
            left: low_first,
            right: low_second,
        } = low_value
        else {
            return Ok(false);
        };
        if !matches!(low_first.as_ref(), Expression::Variable(v) if v == lx)
            || !matches!(low_second.as_ref(), Expression::Variable(v) if v == lx)
        {
            return Ok(false);
        }
        let Expression::Binary {
            operator: BinaryOperator::Subtract,
            left: dec_left,
            right: dec_right,
        } = dec_value
        else {
            return Ok(false);
        };
        if !matches!(dec_left.as_ref(), Expression::Variable(v) if v == iy)
            || !matches!(dec_right.as_ref(), Expression::IntegerLiteral(1))
        {
            return Ok(false);
        }
        // The tail: return hx + iy.
        let Some(Expression::Binary {
            operator: BinaryOperator::Add,
            left: ret_left,
            right: ret_right,
        }) = &function.return_expression
        else {
            return Ok(false);
        };
        if !matches!(ret_left.as_ref(), Expression::Variable(v) if v == hx)
            || !matches!(ret_right.as_ref(), Expression::Variable(v) if v == iy)
        {
            return Ok(false);
        }
        let (Some(hx_register), Some(lx_register), Some(iy_register)) = (
            self.lookup_general(hx),
            self.lookup_general(lx),
            self.lookup_general(iy),
        ) else {
            return Ok(false);
        };
        if hx_register != 3 {
            return Ok(false);
        }
        // The carry temp: next free past the params (r0 holds the bound).
        let temp = 3 + function.parameters.len() as u8;
        if temp > 10 {
            return Ok(false);
        }
        let policy = policy::IntegerLoopPolicy::resolve(self.behavior.integer_loop_style);
        // -- emit --
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 0,
                a: 0,
                immediate: bound_high,
            });
        let test_label = self.fresh_label();
        self.emit_branch_to(test_label);
        let body_label = self.fresh_label();
        self.bind_label(body_label);
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: temp,
                s: lx_register,
                shift: 31,
            });
        if policy.dependency_first {
            self.output.instructions.push(Instruction::Add {
                d: temp,
                a: hx_register,
                b: temp,
            });
            self.output.instructions.push(Instruction::Add {
                d: hx_register,
                a: hx_register,
                b: temp,
            });
            self.output.instructions.push(Instruction::Add {
                d: lx_register,
                a: lx_register,
                b: lx_register,
            });
            self.output.instructions.push(Instruction::AddImmediate {
                d: iy_register,
                a: iy_register,
                immediate: -1,
            });
        } else {
            self.output.instructions.push(Instruction::Add {
                d: lx_register,
                a: lx_register,
                b: lx_register,
            });
            self.output.instructions.push(Instruction::Add {
                d: temp,
                a: hx_register,
                b: temp,
            });
            self.output.instructions.push(Instruction::AddImmediate {
                d: iy_register,
                a: iy_register,
                immediate: -1,
            });
            self.output.instructions.push(Instruction::Add {
                d: hx_register,
                a: hx_register,
                b: temp,
            });
        }
        self.bind_label(test_label);
        self.output.instructions.push(Instruction::CompareWord {
            a: hx_register,
            b: 0,
        });
        self.emit_branch_conditional_to(12, 0, body_label); // blt
        self.output.instructions.push(Instruction::Add {
            d: 3,
            a: hx_register,
            b: iy_register,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += 0;
        Ok(true)
    }

    /// A global-flag while loop over a bare call (`while (gFlag) h();`):
    /// measured @2.6/1.3.2 — the plain non-leaf prologue (nothing crosses in a
    /// register; the flag RELOADS each iteration), the top entry jump to the
    /// bottom test, `bl` body, `lwz r0,@flag (SDA); cmpwi; bne` test, canonical
    /// epilogue.
    pub(crate) fn try_flag_while_loop(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type != Type::Void
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || !function.parameters.is_empty()
            || !function.locals.is_empty()
        {
            return Ok(false);
        }
        let [Statement::Loop {
            kind,
            initializer: None,
            condition: Some(condition),
            step: None,
            body,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if !matches!(kind, LoopKind::While | LoopKind::DoWhile) {
            return Ok(false);
        }
        // The condition: a bare small word global (the SDA reload form).
        let Expression::Variable(flag) = condition else {
            return Ok(false);
        };
        if self.locations.contains_key(flag.as_str())
            || !matches!(
                self.globals.get(flag.as_str()),
                Some(Type::Int | Type::UnsignedInt)
            )
            || self.global_array_sizes.contains_key(flag.as_str())
        {
            return Ok(false);
        }
        // The body: a run of one to three bare direct calls (measured:
        // consecutive bl's, structure unchanged).
        if body.is_empty() || body.len() > 3 {
            return Ok(false);
        }
        let mut body_calls = Vec::with_capacity(body.len());
        for statement in body {
            let Statement::Expression(Expression::Call {
                name: callee,
                arguments,
            }) = statement
            else {
                return Ok(false);
            };
            if !arguments.is_empty()
                || self.locations.contains_key(callee.as_str())
                || self.globals.contains_key(callee.as_str())
                || matches!(
                    self.call_return_types.get(callee.as_str()),
                    Some(Type::Float | Type::Double)
                )
            {
                return Ok(false);
            }
            body_calls.push(callee.clone());
        }
        let flag = flag.clone();

        self.non_leaf = true;
        let plan = mwcc_vreg::FramePlan::sized_for(Vec::new());
        self.frame_size = plan.frame_size;
        // The loop's internal labels advance the @N counter (measured: while 4,
        // do-while 6 — the family constants).
        self.output.anonymous_label_bump += if matches!(kind, LoopKind::DoWhile) {
            6
        } else {
            4
        };
        self.output.instructions.extend(plan.prologue());
        let test = self.fresh_label();
        let loop_body = self.fresh_label();
        // The do-while runs its body first: no top entry jump.
        if matches!(kind, LoopKind::While) {
            self.emit_branch_to(test);
        }
        self.bind_label(loop_body);
        for callee in &body_calls {
            self.emit_call(callee, &[], None, false)?;
        }
        self.bind_label(test);
        self.record_relocation(RelocationKind::EmbSda21, &flag);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, loop_body); // bne
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
