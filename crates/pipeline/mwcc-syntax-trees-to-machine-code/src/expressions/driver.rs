//! The evaluate_general dispatcher, unary emission, float-value classification.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// `(g+h)+x` (recursively): an Add whose LEFT is an Add of two GLOBAL leaves
    /// and whose RIGHT is a register-resident variable — the shape mwcc
    /// reassociates that our source-order emission gets wrong.
    fn global_pair_plus_register(&self, expression: &Expression) -> bool {
        let is_global = |operand: &Expression| {
            matches!(operand, Expression::Variable(name)
                if !self.locations.contains_key(name.as_str()) && self.globals.contains_key(name.as_str()))
        };
        let is_register = |operand: &Expression| {
            matches!(operand, Expression::Variable(name) if self.locations.contains_key(name.as_str()))
        };
        match expression {
            Expression::Binary { operator: BinaryOperator::Add, left, right } => {
                if is_register(right) {
                    if let Expression::Binary { operator: BinaryOperator::Add, left: inner_left, right: inner_right } = left.as_ref() {
                        if is_global(inner_left) && is_global(inner_right) {
                            return true;
                        }
                    }
                }
                self.global_pair_plus_register(left) || self.global_pair_plus_register(right)
            }
            Expression::Binary { left, right, .. } => {
                self.global_pair_plus_register(left) || self.global_pair_plus_register(right)
            }
            Expression::Unary { operand, .. } | Expression::Cast { operand, .. } => self.global_pair_plus_register(operand),
            _ => false,
        }
    }

    pub(crate) fn evaluate_general(&mut self, expression: &Expression, destination: u8) -> Compilation<()> {
        // A compile-time-constant expression — folded constant arithmetic
        // (`2 + 3`, `FLAG_A | FLAG_B`, `1 << 3`) or a side-effect-free identity
        // (`x - x`, `x ^ x`) — materializes the value directly, as mwcc folds it.
        // Bare literals fall through to the arm below.
        if !matches!(expression, Expression::IntegerLiteral(_)) {
            if let Some(value) = constant_value(expression) {
                self.load_integer_constant(destination, value);
                return Ok(());
            }
        }
        // mwcc REASSOCIATES an all-`+` chain of register leaves `v1+v2+…+vN` to `v1 + left-fold(v2..vN)`
        // and allocates it as `add r0,R2,R3; mr R2,R1; <fold R4..R(N-1) into r0>; add D,r0,RN; add D,R2,D`
        // (v1 kept in v2's freed register across the folds that would clobber D). Reproduce it directly
        // for the deferring N>=4 case (N<=3 is byte-exact on the normal path). Each operand must be a
        // DISTINCT register-resident variable — a repeated one is still live, and a constant/global/
        // frame leaf has no ready register — so those fall through to the defer below.
        if let Some(leaves) = crate::analysis::add_chain_leaves(expression) {
            if leaves.len() >= 4 {
                let names: Vec<&str> = leaves.iter().filter_map(|leaf| leaf_name(leaf)).collect();
                let mut sorted = names.clone();
                sorted.sort_unstable();
                sorted.dedup();
                let distinct = names.len() == leaves.len() && sorted.len() == names.len();
                let registers: Option<Vec<u8>> = leaves.iter().map(|leaf| self.general_register_of_leaf(leaf).ok()).collect();
                if let (true, Some(registers)) = (distinct, registers) {
                    if !registers.contains(&GENERAL_SCRATCH) {
                        let last = registers.len() - 1;
                        // v1 must survive the folds, whose last step clobbers the destination. Only
                        // when v1 already sits in the destination register does mwcc move it aside
                        // first — into the LOWER of the two registers the opening `add r0,R2,R3`
                        // just freed; otherwise v1 stays put and is added last.
                        let save_v1 = registers[0] == destination;
                        let save_register = registers[1].min(registers[2]);
                        let v1_register = if save_v1 { save_register } else { registers[0] };
                        self.output.instructions.push(Instruction::Add { d: GENERAL_SCRATCH, a: registers[1], b: registers[2] });
                        if save_v1 {
                            self.output.instructions.push(Instruction::move_register(save_register, registers[0]));
                        }
                        for &register in &registers[3..last] {
                            self.output.instructions.push(Instruction::Add { d: GENERAL_SCRATCH, a: GENERAL_SCRATCH, b: register });
                        }
                        self.output.instructions.push(Instruction::Add { d: destination, a: GENERAL_SCRATCH, b: registers[last] });
                        self.output.instructions.push(Instruction::Add { d: destination, a: v1_register, b: destination });
                        return Ok(());
                    }
                }
            }
        }
        // `(X ± c) + Y` / `Y + (X ± c)` — a `variable ± constant` plus a register-leaf variable —
        // reassociates to `(X + Y) ± c`: mwcc emits `add dest, X, Y; addi dest, dest, ±c` with the
        // `±c` term's variable X ALWAYS the first operand and the leaf Y second, whichever side the
        // `±c` is on (`(a-1)+b` / `b+(a-1)` -> `add r3,r3,r4; addi -1`; `a+(b-1)` -> `add r3,r4,r3;
        // addi -1`). Reproduce it directly. Two register-resident variable leaves + a signed-16
        // constant only; a both-`±c` pair (`(a-1)+(b-1)`, different var order) or a global/memory leaf
        // falls through to the defer below.
        if let Expression::Binary { operator: BinaryOperator::Add, left, right } = expression {
            // Parse a `variable ± constant` term into (var name, constant, inner operator). A nested
            // `fn` (not a closure) so the returned `&str` borrows from the input via lifetime elision.
            fn variable_plus_constant(operand: &Expression) -> Option<(&str, i64, BinaryOperator)> {
                if let Expression::Binary { operator: inner @ (BinaryOperator::Add | BinaryOperator::Subtract), left: il, right: ir } = operand {
                    if let (Expression::Variable(name), Some(constant)) = (il.as_ref(), crate::analysis::constant_value(ir)) {
                        return Some((name.as_str(), constant, *inner));
                    }
                }
                None
            }
            let reassociation = match (variable_plus_constant(left), right.as_ref()) {
                (Some((x, c, op)), Expression::Variable(y)) => Some((x, c, op, y.as_str())),
                _ => match (left.as_ref(), variable_plus_constant(right)) {
                    (Expression::Variable(y), Some((x, c, op))) => Some((x, c, op, y.as_str())),
                    _ => None,
                },
            };
            if let Some((x_name, constant, inner_operator, y_name)) = reassociation {
                let signed = if inner_operator == BinaryOperator::Subtract { constant.checked_neg() } else { Some(constant) };
                if let Some(signed) = signed {
                    if let (Ok(immediate), Some(x_register), Some(y_register)) =
                        (i16::try_from(signed), self.lookup_general(x_name), self.lookup_general(y_name))
                    {
                        self.output.instructions.push(Instruction::Add { d: destination, a: x_register, b: y_register });
                        self.output.instructions.push(Instruction::AddImmediate { d: destination, a: destination, immediate });
                        return Ok(());
                    }
                }
            }
            // BOTH operands `variable ± constant` (`(a-1)+(b-1)`): mwcc groups them with the SECOND
            // term's variable FIRST and sums the (signed) constants — `add r3,r4,r3; addi r3,r3,-2`.
            if let (Some((x1_name, c1, op1)), Some((x2_name, c2, op2))) = (variable_plus_constant(left), variable_plus_constant(right)) {
                let signed = |constant: i64, operator: BinaryOperator| if operator == BinaryOperator::Subtract { constant.checked_neg() } else { Some(constant) };
                if let (Some(s1), Some(s2)) = (signed(c1, op1), signed(c2, op2)) {
                    if let Some(sum) = s1.checked_add(s2) {
                        if let (Ok(immediate), Some(x1_register), Some(x2_register)) =
                            (i16::try_from(sum), self.lookup_general(x1_name), self.lookup_general(x2_name))
                        {
                            self.output.instructions.push(Instruction::Add { d: destination, a: x2_register, b: x1_register });
                            self.output.instructions.push(Instruction::AddImmediate { d: destination, a: destination, immediate });
                            return Ok(());
                        }
                    }
                }
            }
        }
        // `(X + Y) - c` — a two-register sum minus a constant — reassociates to `X + (Y - c)`: mwcc
        // pushes the constant into the SECOND sum operand and adds the first. When the first operand
        // occupies the destination register it is saved to r0 first (`(a+b)-1` -> `mr r0,r3; addi
        // r3,r4,-1; add r3,r0,r3`); otherwise the constant folds in place (`(b+a)-1` -> `addi
        // r3,r3,-1; add r3,r4,r3`). Register-resident variable leaves + signed-16 const; a non-scratch
        // destination only (the r0 save must not alias the destination). Nested/global cases defer.
        if destination != GENERAL_SCRATCH {
            if let Expression::Binary { operator: BinaryOperator::Subtract, left, right } = expression {
                if let (Expression::Binary { operator: BinaryOperator::Add, left: inner_left, right: inner_right }, Some(constant)) =
                    (left.as_ref(), crate::analysis::constant_value(right))
                {
                    if let (Expression::Variable(first_name), Expression::Variable(second_name)) = (inner_left.as_ref(), inner_right.as_ref()) {
                        if let Some(negated) = constant.checked_neg() {
                            if let (Ok(immediate), Some(first_register), Some(second_register)) =
                                (i16::try_from(negated), self.lookup_general(first_name), self.lookup_general(second_name))
                            {
                                if first_register == destination {
                                    self.output.instructions.push(Instruction::move_register(GENERAL_SCRATCH, first_register));
                                    self.output.instructions.push(Instruction::AddImmediate { d: destination, a: second_register, immediate });
                                    self.output.instructions.push(Instruction::Add { d: destination, a: GENERAL_SCRATCH, b: destination });
                                } else {
                                    self.output.instructions.push(Instruction::AddImmediate { d: destination, a: second_register, immediate });
                                    self.output.instructions.push(Instruction::Add { d: destination, a: first_register, b: destination });
                                }
                                return Ok(());
                            }
                        }
                    }
                }
            }
        }
        // Other reassociated add-trees (nested non-leaf operands, mixed with `*`) still diverge in
        // register allocation — defer rather than emit wrong bytes (#20 allocator).
        if crate::analysis::contains_complex_add(expression) {
            return Err(Diagnostic::error("a reassociated integer add-tree needs the keystone allocator (roadmap)"));
        }
        // `(g+h)+x` — a two-GLOBAL inner sum plus a register leaf: mwcc reassociates
        // the register operand INTO the first add (`lwz g; lwz h; add r3,g,x; add
        // r3,h,r3`) while source order sums the globals first — wrong bytes. The
        // sibling spellings (g+h+k, (a+b)+g, x+(g+h)) already defer; this left-chain
        // escaped the leaf-shape checks (globals and registers both parse as Variable).
        if self.global_pair_plus_register(expression) {
            return Err(Diagnostic::error("a global-pair sum joined with a register value needs the keystone allocator (roadmap)"));
        }
        // `((a*b)+1)*((c*d)+2)` — BOTH multiply operands are (mul + const): mwcc's
        // allocator orients the intermediates opposite to ours (first mullw -> r4,
        // second -> r3, measured) — wrong bytes. The single-addend form matches;
        // defer the both-addend sibling until the allocator models it.
        if let Expression::Binary { operator: BinaryOperator::Multiply, left, right } = expression {
            let mul_plus_const = |operand: &Expression| {
                matches!(operand, Expression::Binary { operator: BinaryOperator::Add | BinaryOperator::Subtract, left: inner, right: constant }
                    if matches!(inner.as_ref(), Expression::Binary { operator: BinaryOperator::Multiply, .. })
                        && crate::analysis::constant_value(constant).is_some())
            };
            if mul_plus_const(left) && mul_plus_const(right) {
                return Err(Diagnostic::error("a product of two addend-carrying products needs the keystone allocator (roadmap)"));
            }
        }
        // mwcc keeps a constant-amount shift as the FIRST operand of a commutative op (`(a<<2)+b` ->
        // `add d, shift, b`), but our placement swaps it to second (like `(a*4)+b`). Defer the
        // ordering rather than emit swapped bytes; matching it is the keystone allocator's job.
        if crate::analysis::contains_commutative_shift_left(expression) {
            return Err(Diagnostic::error("a commutative op with a constant-shift left operand orders operands differently (roadmap)"));
        }
        match expression {
            // A compound-literal VALUE needs the frame-temporary + copy schedule.
            Expression::CompoundLiteral { .. } => Err(Diagnostic::error(
                "a compound-literal argument needs the frame-temporary schedule (roadmap)",
            )),
            Expression::CallThrough { .. } => Err(Diagnostic::error(
                "an indirect call through a member function pointer is not supported here (captures only)",
            )),
            Expression::AggregateLiteral(_) => Err(Diagnostic::error("an aggregate initializer is not supported here (captures only)")),
            Expression::PostStep { .. } => Err(Diagnostic::error(
                "a postfix step used as a value is not supported yet (roadmap)",
            )),
            Expression::IntegerLiteral(value) => {
                self.load_integer_constant(destination, *value);
                Ok(())
            }
            Expression::StringLiteral(bytes) => self.emit_string_literal(bytes, destination),
            // `&x` for a frame-resident variable is its address: `addi d, r1, slot`.
            Expression::AddressOf { operand } => self.emit_address_of(operand, destination),
            // `m[k]` on a flattened MULTI-DIM frame array is the ROW ADDRESS
            // `addi d, r1, slot + k*row_bytes` (measured: g(m[2]) -> addi r3,r1,40
            // for float m[3][4] at slot 8). A one-dim frame array's m[k] is a VALUE
            // and falls through to the general Index handling below.
            Expression::Index { base, index }
                if matches!(base.as_ref(), Expression::Variable(name)
                    if self.frame_row_bytes.contains_key(name.as_str()))
                    && constant_value(index).is_some() =>
            {
                let Expression::Variable(name) = base.as_ref() else { unreachable!() };
                let row_bytes = self.frame_row_bytes[name.as_str()];
                let row = constant_value(index).unwrap();
                let slot = self
                    .frame_slots
                    .get(name.as_str())
                    .copied()
                    .ok_or_else(|| Diagnostic::error("a row access on an unallocated frame array (roadmap)"))?;
                let offset = slot.offset as i64 + row * row_bytes as i64;
                if offset < i16::MIN as i64 || offset > i16::MAX as i64 {
                    return Err(Diagnostic::error("a frame-array row offset is out of range (roadmap)"));
                }
                self.output.instructions.push(Instruction::AddImmediate { d: destination, a: 1, immediate: offset as i16 });
                Ok(())
            }
            Expression::Variable(name) => {
                // A frame-resident variable is reloaded from its stack slot; a local
                // array decays to the slot's address instead (`addi d,r1,offset`).
                if let Some(slot) = self.frame_slots.get(name).copied() {
                    if slot.is_array {
                        self.output.instructions.push(Instruction::AddImmediate { d: destination, a: 1, immediate: slot.offset });
                    } else {
                        self.output.instructions.push(Instruction::LoadWord { d: destination, a: 1, offset: slot.offset });
                    }
                    return Ok(());
                }
                if let Some(location) = self.locations.get(name) {
                    if location.class != ValueClass::General {
                        return Err(Diagnostic::error(format!("'{name}' is not an integer")));
                    }
                    let (source, width, signed) = (location.register, location.width, location.signed);
                    self.emit_widen(destination, source, width, signed);
                    Ok(())
                } else if let Some(&total_size) = self.global_array_sizes.get(name.as_str()).filter(|_| destination != GENERAL_SCRATCH) {
                    // A bare array variable in value position decays to its address
                    // (`return g;` / `f(g)` for `int g[N]`), not a load of g[0].
                    self.emit_global_array_base(name, total_size, destination)
                } else {
                    self.emit_global_load(name, destination)
                }
            }
            Expression::Unary { operator, operand } => {
                // Negating a COMPARISON value — mwcc folds the negate into the comparison's bool
                // idiom. The SIGN-BIT comparisons `-(x < 0)` / `-(x > 0)` are modeled byte-exactly
                // (a 0/-1 via neg/andc/srawi), but every OTHER negated comparison (`-(a < b)`,
                // `-(a == 0)`, against a non-zero constant) uses a fused srawi/rlwinm we don't model,
                // whereas ours emits the 0/1 value and a separate `neg` — a byte-different sequence.
                if *operator == UnaryOperator::Negate {
                    if let Expression::Binary { operator: inner, right, .. } = operand.as_ref() {
                        let is_sign_bit_comparison = matches!(inner, BinaryOperator::Less | BinaryOperator::Greater)
                            && crate::analysis::constant_value(right) == Some(0);
                        if is_comparison(*inner) && !is_sign_bit_comparison {
                            return Err(Diagnostic::error("negating a comparison value uses a fused bool idiom not modeled (roadmap)"));
                        }
                    }
                }
                self.emit_unary(*operator, operand, destination)
            }
            Expression::Conditional { condition, when_true, when_false } => {
                self.emit_conditional(condition, when_true, when_false, destination, false)
            }
            Expression::Cast { target_type, operand } => self.emit_cast_to_integer(*target_type, operand, destination),
            Expression::Dereference { pointer } => self.emit_load_from_pointer(pointer, destination),
            Expression::Member { base, offset, member_type, index_stride } => self.emit_member_load(base, *offset, *member_type, *index_stride, destination),
            Expression::MemberAddress { base, offset, .. } => {
                // The array's address: `base + offset` (a `mr` when the array is at
                // the start of the struct).
                let base_register = self.member_base_register(base)?;
                if *offset == 0 {
                    if base_register != destination {
                        self.output.instructions.push(Instruction::move_register(destination, base_register));
                    }
                } else {
                    self.output.instructions.push(Instruction::AddImmediate { d: destination, a: base_register, immediate: *offset as i16 });
                }
                Ok(())
            }
            Expression::Index { base, index } => self.emit_subscript(base, index, destination),
            Expression::Call { name, arguments } => self.emit_call(name, arguments, Some(destination), false),
            Expression::Assign { target, value } => self.emit_assign(target, value, destination),
            // The comma operator is byte-exact only in proven value positions: a store
            // value (peeled in `place_store_value`) and a flat-arithmetic binary operand
            // (peeled in the `Binary` arm above). Reaching here means a risky sub-operand
            // position (unary operand, cast, return value, …) where forcing the right into
            // `destination` adds a move mwcc elides — defer rather than ship that diff.
            Expression::Comma { .. } => {
                let _ = destination;
                Err(Diagnostic::error("a comma operator in this position is not supported yet (roadmap)"))
            }
            Expression::Binary { operator, left, right } => {
                // mwcc folds a unary minus into a subtract: `-a + b` -> `b - a` (subf), `a + -b` ->
                // `a - b`. Rewrite to the equivalent Subtract so the byte-exact subtract path emits it
                // (the operand order matches: `subf d, a, b` computes b - a).
                if *operator == BinaryOperator::Add {
                    if let Expression::Unary { operator: UnaryOperator::Negate, operand } = left.as_ref() {
                        let rewritten = Expression::Binary { operator: BinaryOperator::Subtract, left: right.clone(), right: operand.clone() };
                        return self.evaluate_general(&rewritten, destination);
                    }
                    if let Expression::Unary { operator: UnaryOperator::Negate, operand } = right.as_ref() {
                        let rewritten = Expression::Binary { operator: BinaryOperator::Subtract, left: left.clone(), right: operand.clone() };
                        return self.evaluate_general(&rewritten, destination);
                    }
                }
                // `(cmp1) OP (cmp2)` — an arithmetic/bitwise combine of TWO comparison-as-value
                // idioms (`(a>0) - (a<0)`, `(a<b) + (a>b)`, `(a>0) | (a<0)`). mwcc INTERLEAVES the
                // two comparison computations (instruction scheduling + register allocation); ours
                // evaluates them sequentially — a correct value but a byte-different order. Matching
                // the schedule needs the register allocator, so defer. (A logical `&&`/`||` combine
                // routes through the short-circuit path; a comparison mixed with a leaf/constant —
                // `(a<b) - 1` — keeps its single-comparison shape and is unaffected.)
                if !is_comparison(*operator)
                    && !matches!(operator, BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr)
                    && matches!(left.as_ref(), Expression::Binary { operator: inner, .. } if is_comparison(*inner))
                    && matches!(right.as_ref(), Expression::Binary { operator: inner, .. } if is_comparison(*inner))
                {
                    return Err(Diagnostic::error("an arithmetic combine of two comparison values needs the register allocator's interleaving (roadmap)"));
                }
                // `(cmp) << k` and `(cmp) & k` — mwcc fuses the comparison's sign-bit extract with
                // the shift into one `rlwinm`, and drops a `& k` mask that is redundant on a 0/1
                // value. Neither peephole is modeled, so defer. (Other operators on a comparison —
                // `| ^ >> + - *` — keep the plain comparison-value shape and stay byte-exact.)
                if matches!(operator, BinaryOperator::ShiftLeft | BinaryOperator::BitAnd)
                    && (matches!(left.as_ref(), Expression::Binary { operator: inner, .. } if is_comparison(*inner))
                        || matches!(right.as_ref(), Expression::Binary { operator: inner, .. } if is_comparison(*inner)))
                {
                    return Err(Diagnostic::error("a shift-left or bitwise-and of a comparison value uses a fused rlwinm / dropped mask not modeled (roadmap)"));
                }
                // A repeated NON-LEAF sub-expression (`(a+1)+(a+1)`, `(a + (a>>31)) ^ (a>>31)`) is a
                // common sub-expression mwcc computes ONCE and reuses; our straight-line codegen
                // recomputes it — a byte-different sequence. Defer until the register allocator does
                // CSE. (Leaf repeats like `a + a` are cheap re-reads and stay byte-exact.)
                if crate::analysis::has_repeated_nonleaf_subexpression(expression) {
                    return Err(Diagnostic::error("a repeated common sub-expression needs the register allocator's CSE (roadmap)"));
                }
                // A repeated GLOBAL variable leaf (`gi + gi`, `gi * gi`): unlike a register-resident
                // parameter/local (a free re-read), a global read is a LOAD, and mwcc loads it ONCE,
                // reusing the register (`lwz r0,gi; mullw r3,r0,r0`). The self-folding ops (`& | ^ - / %`
                // and comparisons: `g OP g` -> `g`, `0`, or `1`) collapse before this; the non-folding
                // `+ * << >>` reach here. Reproduce mwcc's load-once for `+`/`*` (load into the scratch,
                // then the op twice); the shifts' `slw`/`sraw` operand handling for this shape isn't
                // modeled, so they defer.
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Multiply | BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight) {
                    if let (Expression::Variable(left_name), Expression::Variable(right_name)) = (left.as_ref(), right.as_ref()) {
                        if left_name == right_name && self.globals.contains_key(left_name.as_str()) {
                            // A signed plain `char` global reads with `lbz` then a trailing `extsb` that
                            // this load-once path does not emit (`lha`/`lwz` and the unsigned loads
                            // self-extend, so they are fine). Defer the signed-char case — rare.
                            if matches!(self.globals.get(left_name.as_str()), Some(Type::Char)) {
                                return Err(Diagnostic::error("a repeated signed-char global read needs the sign-extended load-once (roadmap)"));
                            }
                            match operator {
                                BinaryOperator::Add => {
                                    self.emit_global_load_value(left_name, GENERAL_SCRATCH)?;
                                    self.output.instructions.push(Instruction::Add { d: destination, a: GENERAL_SCRATCH, b: GENERAL_SCRATCH });
                                    return Ok(());
                                }
                                BinaryOperator::Multiply => {
                                    self.emit_global_load_value(left_name, GENERAL_SCRATCH)?;
                                    self.output.instructions.push(Instruction::MultiplyLow { d: destination, a: GENERAL_SCRATCH, b: GENERAL_SCRATCH });
                                    return Ok(());
                                }
                                _ => return Err(Diagnostic::error("a repeated global read under a shift needs load-once reuse (roadmap)")),
                            }
                        }
                    }
                }
                // `gp->x OP gp->y` — a GLOBAL POINTER dereferenced on BOTH sides (any op, any members):
                // the pointer is a LOAD, and our per-operand access reloads it each time (`lwz r3,gp;
                // lwz r3,0(r3); lwz r0,gp; lwz r0,4(r0)`), while mwcc loads the pointer ONCE and reads
                // both members from it (`lwz r4,gp; lwz r3,0(r4); lwz r0,4(r4)`). Defer — a register-
                // resident pointer parameter, or two DIFFERENT global pointers, load correctly.
                {
                    fn deref_base(operand: &Expression) -> Option<&str> {
                        let base = match operand {
                            Expression::Member { base, .. } | Expression::Index { base, .. } => base.as_ref(),
                            Expression::Dereference { pointer } => pointer.as_ref(),
                            _ => return None,
                        };
                        match base {
                            Expression::Variable(name) => Some(name.as_str()),
                            _ => None,
                        }
                    }
                    if let (Some(left_base), Some(right_base)) = (deref_base(left), deref_base(right)) {
                        if left_base == right_base && self.globals.contains_key(left_base) {
                            return Err(Diagnostic::error("a global pointer dereferenced on both sides needs load-once reuse (roadmap)"));
                        }
                    }
                }
                // A comma operand with a side-effect-free left is equivalent to its right
                // value; peel it so the right keeps its natural register (`(a,b)+1` == `b+1`,
                // no spurious move). Only a flat arithmetic binary of leaves/constants is
                // provably byte-exact this way — comparisons and computed operands route to
                // codegen shapes with pre-existing gaps, so those (and a side-effecting left)
                // defer rather than ship a guess.
                if matches!(left.as_ref(), Expression::Comma { .. }) || matches!(right.as_ref(), Expression::Comma { .. }) {
                    let peel = |operand: &Expression| -> Compilation<Expression> {
                        let mut current = operand;
                        while let Expression::Comma { left, right } = current {
                            if expression_has_side_effect(left) {
                                return Err(Diagnostic::error("a comma operand with a side effect is not supported yet (roadmap)"));
                            }
                            current = right;
                        }
                        Ok(current.clone())
                    };
                    let (peeled_left, peeled_right) = (peel(left)?, peel(right)?);
                    let is_simple = |operand: &Expression| {
                        matches!(operand, Expression::Variable(_) | Expression::IntegerLiteral(_) | Expression::FloatLiteral(_))
                    };
                    if is_comparison(*operator)
                        || matches!(operator, BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr)
                        || !is_simple(&peeled_left)
                        || !is_simple(&peeled_right)
                    {
                        return Err(Diagnostic::error("a comma operand in this expression is not supported yet (roadmap)"));
                    }
                    let peeled = Expression::Binary {
                        operator: *operator,
                        left: Box::new(peeled_left),
                        right: Box::new(peeled_right),
                    };
                    return self.evaluate_general(&peeled, destination);
                }
                // Fold `(x OP c1) OP c2` to `x OP (c1 ⊕ c2)` for an associative operation with a
                // constant at each level — mwcc combines consecutive constant operations into a
                // single instruction: `(a+3)+5` is `addi r3,r3,8`, `(a>>2)>>3` is `srawi
                // r3,r3,5`, `(a&0xf0)&0x3c` is one `rlwinm`. Rewrite and re-evaluate so the
                // existing single-constant path emits it. (Left-associative trees keep the
                // constant on the inner/outer right, the common shape.)
                if let Some(outer_constant) = constant_value(right) {
                    if let Expression::Binary { operator: inner_operator, left: inner_left, right: inner_right } = left.as_ref() {
                        if let Some(inner_constant) = constant_value(inner_right) {
                            use BinaryOperator::*;
                            let folded = match (*operator, *inner_operator) {
                                (Add, Add) => Some((Add, inner_constant + outer_constant)),
                                (Subtract, Add) => Some((Add, inner_constant - outer_constant)),
                                (Add, Subtract) => Some((Add, outer_constant - inner_constant)),
                                (Subtract, Subtract) => Some((Subtract, inner_constant + outer_constant)),
                                (Multiply, Multiply) => inner_constant.checked_mul(outer_constant).map(|product| (Multiply, product)),
                                (BitAnd, BitAnd) => Some((BitAnd, inner_constant & outer_constant)),
                                (BitOr, BitOr) => Some((BitOr, inner_constant | outer_constant)),
                                (BitXor, BitXor) => Some((BitXor, inner_constant ^ outer_constant)),
                                (ShiftLeft, ShiftLeft) | (ShiftRight, ShiftRight)
                                    if (1..=31).contains(&(inner_constant + outer_constant)) =>
                                    Some((*operator, inner_constant + outer_constant)),
                                _ => None,
                            };
                            if let Some((result_operator, result_constant)) = folded {
                                let folded_expression = Expression::Binary {
                                    operator: result_operator,
                                    left: inner_left.clone(),
                                    right: Box::new(Expression::IntegerLiteral(result_constant)),
                                };
                                return self.evaluate_general(&folded_expression, destination);
                            }
                        }
                    }
                }
                // Operand cancellation: `(X + Y) - Y` is `X`, `(X + Y) - X` is `Y`, and
                // `(X - Y) + Y` is `X` — the operand that appears with opposite signs cancels,
                // and mwcc folds straight to the survivor (`(a+b)-b` is `blr`; `(a+5)-a` is
                // `li r3,5`). The cancelling operand must be a side-effect-free LEAF (a variable
                // or constant — read twice gives the same value), so dropping its evaluation is
                // safe; the survivor is then evaluated normally.
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract) {
                    if matches!(right.as_ref(), Expression::Variable(_) | Expression::IntegerLiteral(_)) {
                        if let Expression::Binary { operator: inner_operator, left: inner_left, right: inner_right } = left.as_ref() {
                            let survivor = match (*operator, *inner_operator) {
                                (BinaryOperator::Subtract, BinaryOperator::Add) => {
                                    if same_operand(right, inner_right) { Some(inner_left) }
                                    else if same_operand(right, inner_left) { Some(inner_right) }
                                    else { None }
                                }
                                (BinaryOperator::Add, BinaryOperator::Subtract) if same_operand(right, inner_right) => Some(inner_left),
                                _ => None,
                            };
                            if let Some(survivor) = survivor {
                                return self.evaluate_general(survivor, destination);
                            }
                        }
                    }
                }
                // Absorption: `(a & b) | a` is `a`, `(a | b) & a` is `a` (and the commuted
                // `a | (a & b)`, plus the bit-constant forms `(x | c) & c` -> c, `(x & c) | c`
                // -> c when the survivor is the constant). One operand subsumes the other and
                // mwcc folds straight to the survivor (`(a&b)|a` is a bare `blr`; `(x|7)&7` is
                // `li r3,7`). The inner op is dropped, so its operands — and the surviving leaf
                // — must be side-effect-free leaves (variables/constants) for the fold to hold.
                if matches!(operator, BinaryOperator::BitOr | BinaryOperator::BitAnd) {
                    let dual = if matches!(operator, BinaryOperator::BitOr) { BinaryOperator::BitAnd } else { BinaryOperator::BitOr };
                    for (inner, survivor) in [(left.as_ref(), right.as_ref()), (right.as_ref(), left.as_ref())] {
                        let survivor_is_leaf = matches!(survivor, Expression::Variable(_) | Expression::IntegerLiteral(_));
                        if let Expression::Binary { operator: inner_operator, left: p, right: q } = inner {
                            let inner_leaves = matches!(p.as_ref(), Expression::Variable(_) | Expression::IntegerLiteral(_))
                                && matches!(q.as_ref(), Expression::Variable(_) | Expression::IntegerLiteral(_));
                            if survivor_is_leaf && *inner_operator == dual && inner_leaves
                                && (same_operand(survivor, p) || same_operand(survivor, q)) {
                                return self.evaluate_general(survivor, destination);
                            }
                        }
                    }
                }
                // `a*c1 + a*c2` / `a*c1 - a*c2` on the same variable distributes to `a*(c1±c2)`
                // — mwcc combines the like terms before strength reduction (`a*3 + a*5` is one
                // `slwi r3,r3,3` for `a*8`, not two `mulli`s). Fold and re-evaluate so the
                // existing `a*const` strength reduction emits the single instruction.
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract) {
                    let as_variable_times_constant = |expression: &Expression| -> Option<(String, i64)> {
                        if let Expression::Binary { operator: BinaryOperator::Multiply, left, right } = expression {
                            if let (Expression::Variable(name), Some(constant)) = (left.as_ref(), constant_value(right)) {
                                return Some((name.clone(), constant));
                            }
                            if let (Some(constant), Expression::Variable(name)) = (constant_value(left), right.as_ref()) {
                                return Some((name.clone(), constant));
                            }
                        }
                        None
                    };
                    if let (Some((left_variable, left_constant)), Some((right_variable, right_constant))) =
                        (as_variable_times_constant(left), as_variable_times_constant(right))
                    {
                        let combined = if *operator == BinaryOperator::Add { left_constant + right_constant } else { left_constant - right_constant };
                        // mwcc folds `a*c1 ± a*c2` to a single `slwi` only in a narrow shape:
                        // both factors ODD and ≥ 3, distinct, with the combined factor a power
                        // of two ≥ 2. Each odd factor would otherwise be its own `mulli`, so
                        // collapsing them to one shift (`a*3 + a*5` -> `slwi r3,r3,3`) is mwcc's
                        // win. An even factor (`a*2`, itself shift-cheap), a factor of 1 (really
                        // `a`), identical terms (CSE'd), or a non-power-of-two sum (a `mulli`
                        // result) are NOT folded — they keep their existing lowering.
                        if left_variable == right_variable
                            && left_constant % 2 == 1
                            && right_constant % 2 == 1
                            && left_constant >= 3
                            && right_constant >= 3
                            && left_constant != right_constant
                            && combined >= 2
                            && (combined as u64).is_power_of_two()
                        {
                            let folded = Expression::Binary {
                                operator: BinaryOperator::Multiply,
                                left: Box::new(Expression::Variable(left_variable)),
                                right: Box::new(Expression::IntegerLiteral(combined)),
                            };
                            return self.evaluate_general(&folded, destination);
                        }
                    }
                }
                // `(a-b)-(c-d)`, `a*b-c*d`: a SUBTRACT whose BOTH operands are computed binary
                // expressions evaluates the two sub-trees in an order/allocation our straight-line path
                // does not match mwcc's (unlike `+`, subtraction is not commutative, so it cannot reuse
                // the overlap idiom). Defer until the keystone allocator schedules it; a leaf/constant
                // operand keeps the byte-exact single-scratch shape (`a-b-c`, `a-(b-c)`). Placed AFTER
                // the distributive fold so `a*5 - a*3` collapses to `a*2` first.
                if *operator == BinaryOperator::Subtract
                    && matches!(left.as_ref(), Expression::Binary { .. })
                    && matches!(right.as_ref(), Expression::Binary { .. })
                {
                    return Err(Diagnostic::error("a subtract of two computed sub-expressions needs the keystone allocator (roadmap)"));
                }
                // A signed char load (member `p->x`, element `a[i]`, deref `*p`) that is a
                // DIRECT operand of a comparison or a signed divide is loaded raw by these
                // branchless idioms — `p->x > 0` / `p->x / 2` operate on the zero-extended byte
                // where mwcc sign-extends first (`lbz r0; extsb r3,r0; <idiom>`), a miscompile
                // for a negative value. The byte-exact form needs mwcc's r0-load register
                // choice (the keystone allocator), so defer. (A masked operand `(p->x & 0xf) >
                // 0` is not a direct load and is unaffected; a short load sign-extends.)
                if is_comparison(*operator) || matches!(operator, BinaryOperator::Divide) {
                    if self.is_signed_byte_load(left.as_ref())? || self.is_signed_byte_load(right.as_ref())? {
                        // `signed_char < 0` is the 1-instruction sign-bit idiom; comparisons.rs loads
                        // the byte into the scratch and sign-extends it in place. The other relations
                        // (and divide) still need per-case handling, so defer them.
                        let against_zero = matches!(operator, BinaryOperator::Less | BinaryOperator::Greater | BinaryOperator::GreaterEqual | BinaryOperator::NotEqual | BinaryOperator::Equal | BinaryOperator::LessEqual)
                            && is_zero_literal(right.as_ref());
                        // `signed_char == c` (any small constant): `lbz r0; extsb r0,r0; subfic;
                        // cntlzw; srwi` — handled by the a==c leading-zeros idiom.
                        let equal_constant = matches!(operator, BinaryOperator::Equal)
                            && as_small_integer(right.as_ref()).is_some();
                        let handled = (against_zero || equal_constant)
                            && self.is_signed_byte_load(left.as_ref())?;
                        if !handled {
                            return Err(Diagnostic::error("a signed char load operand of a comparison/divide needs a sign-extension (roadmap)"));
                        }
                    }
                }
                // Comparisons compile to branchless idioms.
                if is_comparison(*operator) {
                    return self.emit_comparison(*operator, left, right, destination);
                }
                // Short-circuit `&&`/`||` as a value (a store, an operand) builds its
                // 0/1 result with forward branches through the scratch and a join, vs the
                // tail form's early `beqlr` returns.
                if matches!(operator, BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr) {
                    return self.emit_short_circuit_via_scratch(*operator, left, right, destination);
                }
                // `&global +/- n` is pointer arithmetic: materialize the address into a
                // fresh register, then add the offset scaled by the pointee size
                // (`&ga + 1` is `addi d,&ga,4`). Add is commutative; subtract is ptr-int.
                // A variable index or an offset that overflows the `addi` immediate defers.
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract) {
                    let address = if self.is_global_address_of(left) {
                        Some((left, right))
                    } else if *operator == BinaryOperator::Add && self.is_global_address_of(right) {
                        Some((right, left))
                    } else {
                        None
                    };
                    if let Some((address, offset)) = address {
                        if let (Expression::AddressOf { operand: inner }, Some(count)) = (address.as_ref(), constant_value(offset)) {
                            let size = match inner.as_ref() {
                                Expression::Variable(name) => self.globals.get(name.as_str()).map(|global| (global.width() / 8) as i64),
                                _ => None,
                            };
                            let scaled = size.map(|size| count * size * if *operator == BinaryOperator::Subtract { -1 } else { 1 });
                            if let Some(Ok(immediate)) = scaled.map(i16::try_from) {
                                // mwcc materializes the address in the lowest free register
                                // then adds the offset: the destination in place when it is
                                // a real register (`addi r3,r3,n` for a return or call arg),
                                // else a fresh register (`li r3,0; addi r0,r3,n` for a store).
                                let address_register = if destination == GENERAL_SCRATCH {
                                    self.fresh_virtual_general()
                                } else {
                                    destination
                                };
                                self.emit_address_of(inner, address_register)?;
                                self.output.instructions.push(Instruction::AddImmediate { d: destination, a: address_register, immediate });
                                return Ok(());
                            }
                        }
                        return Err(Diagnostic::error("pointer arithmetic on a global's address needs offset scaling (roadmap)"));
                    }
                }
                // Identical simple loads on both sides (`*p op *p`, `a[0]+a[0]`):
                // mwcc loads the value ONCE and folds operator identities, rather
                // than the two-operand double load.
                if self.try_emit_identical_load_binary(*operator, left, right, destination)? {
                    return Ok(());
                }
                // Negation folds in a subtraction: `X - (-Y)` is `X + Y`, and
                // `(-a) - Y` (non-constant Y) is `-(a + Y)` — mwcc cancels the
                // double negative / hoists the negate over the sum.
                if *operator == BinaryOperator::Subtract {
                    if let Expression::Unary { operator: UnaryOperator::Negate, operand: inner } = right.as_ref() {
                        let sum = Expression::Binary { operator: BinaryOperator::Add, left: left.clone(), right: inner.clone() };
                        return self.evaluate_general(&sum, destination);
                    }
                    if let Expression::Unary { operator: UnaryOperator::Negate, operand: inner } = left.as_ref() {
                        if constant_value(right).is_none() {
                            let sum = Expression::Binary { operator: BinaryOperator::Add, left: inner.clone(), right: right.clone() };
                            let negated = Expression::Unary { operator: UnaryOperator::Negate, operand: Box::new(sum) };
                            return self.evaluate_general(&negated, destination);
                        }
                    }
                }
                // A shift fused with a mask — `(x >> n) & m`, `(x & m) << n`, etc. —
                // is a single rotate-and-mask (`rlwinm`). Caught before the per-shift
                // paths so the fused form wins over a plain shift.
                if matches!(operator, BinaryOperator::BitAnd | BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight)
                    && self.try_emit_rotate_mask(*operator, left, right, destination)?
                {
                    return Ok(());
                }
                // Right shift, divide, and modulo select instructions by signedness.
                if *operator == BinaryOperator::ShiftRight {
                    return self.emit_shift_right(left, right, destination);
                }
                if *operator == BinaryOperator::Divide {
                    return self.emit_divide(left, right, destination);
                }
                if *operator == BinaryOperator::Modulo {
                    return self.emit_modulo(left, right, destination);
                }
                // Pointer arithmetic scales the integer operand by the pointee
                // size (e.g. `int* + i` -> `slwi i,2; add`); byte pointers need no
                // scaling and fall through to plain addition.
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract)
                    && self.try_emit_pointer_arithmetic(*operator, left, right, destination)?
                {
                    return Ok(());
                }
                // `x & ~y` / `x | ~y` fuse into andc/orc.
                if matches!(operator, BinaryOperator::BitAnd | BinaryOperator::BitOr)
                    && self.try_emit_complement_logical(*operator, left, right, destination)
                {
                    return Ok(());
                }
                // `(x & m1) | (x & m2)` for the same leaf folds to `x & (m1|m2)`.
                if matches!(operator, BinaryOperator::BitOr)
                    && self.try_emit_same_leaf_mask_or(left, right, destination)?
                {
                    return Ok(());
                }
                // A VARIABLE rotate `(a<<n)|(a>>(32-n))` (or the mirror right rotate) folds to one
                // `rotlw` (`rlwnm ...,0,31`), with a `subfic` first for the right-rotate amount.
                if matches!(operator, BinaryOperator::BitOr)
                    && self.try_emit_variable_rotate(left, right, destination)?
                {
                    return Ok(());
                }
                // An OR of two complementary bit fields (shifts and/or masks) —
                // including a constant rotate — merges via one rlwimi.
                if matches!(operator, BinaryOperator::BitOr)
                    && self.try_emit_field_merge(left, right, destination)?
                {
                    return Ok(());
                }
                // The same merge where the operands are memory loads (the pointer-pun
                // `__HI`/`__LO` merge): load both, then rlwimi.
                if matches!(operator, BinaryOperator::BitOr)
                    && self.try_emit_field_merge_loads(left, right, destination)?
                {
                    return Ok(());
                }
                // mwcc reassociates a left-leaning pure-addition chain before the
                // generic paths see it (`a+b+c` -> `a+(b+c)`).
                if *operator == BinaryOperator::Add
                    && self.try_emit_additive_chain(left, right, destination)?
                {
                    return Ok(());
                }
                // `a*b + a*c` / `a*b - a*c` distribute to `a*(b±c)` — one `add`/`subf` then one `mullw`.
                if self.try_emit_distributed_product(*operator, left, right, destination)? {
                    return Ok(());
                }
                // `(cond ? c1 : c2) +/- k` distributes the constant into the arms
                // (mwcc folds the select's trailing `addi`).
                if self.try_emit_select_constant_fold(*operator, left, right, destination)? {
                    return Ok(());
                }
                // A 16-bit constant operand folds into an immediate instruction.
                if self.try_emit_general_with_constant(*operator, left, right, destination)? {
                    return Ok(());
                }
                // Two memory loads from a common base (`a[i] op a[j]`, `s->x op s->y`)
                // — the first multi-operand shape on the register allocator.
                if self.try_emit_two_load_binary(*operator, left, right, destination)? {
                    return Ok(());
                }
                // A constant-index subscript load paired with a wide-int leaf
                // (`a[k] op x`) — the load goes to the scratch like a dereference.
                if self.try_emit_subscript_leaf_binary(*operator, left, right, destination)? {
                    return Ok(());
                }
                // `(x & y) | (x & z)` and the other bitwise distributive laws collapse to
                // `x & (y | z)` (one fewer instruction), as mwcc does.
                if self.try_emit_distributive_bitwise(*operator, left, right, destination)? {
                    return Ok(());
                }
                // Two COMPOUND-load operands (each wrapping a load in an op, `p->x*p->x +
                // p->y*p->y`) reach the generic combine only as genuinely complex shapes — the
                // simple two-load idioms above already took `*p op *q`, `s->x op s->y`, and two BARE
                // loads (`*(p+1)+*(p+2)`) stay byte-exact (loads adjacent). mwcc hoists both loads to
                // the top with an allocator-chosen register assignment; the generic combine
                // interleaves load/op/load/op — same result, different schedule. Defer, don't ship.
                if is_compound_load(left) && is_compound_load(right) {
                    return Err(Diagnostic::error("a binary over two compound-load operands needs the allocator (roadmap)"));
                }
                if !fits_single_scratch(expression, destination == GENERAL_SCRATCH) {
                    return Err(Diagnostic::error("expression needs the full register allocator (roadmap M1)"));
                }
                let operands = self.place_general_operands(*operator, left, right)?;
                self.output.instructions.push(general_combine(*operator, destination, operands)?);
                Ok(())
            }
            Expression::FloatLiteral(_) => Err(Diagnostic::error("float literal in integer context")),
        }
    }

    /// Whether an expression yields a float (a float leaf, literal, or load).
    pub(crate) fn is_float_value(&self, expression: &Expression) -> bool {
        match expression {
            Expression::FloatLiteral(_) => true,
            Expression::Variable(_) => self.is_float_leaf(expression),
            Expression::Dereference { pointer } => matches!(self.pointee_of(pointer), Ok(Pointee::Float | Pointee::Double)),
            Expression::Index { base, .. } => {
                // A pointer/array element whose pointee is float/double — OR an element of a
                // file-scope float/double array (whose base is not in `locations`, so `pointee_of`
                // can't classify it; consult the global's element type directly).
                matches!(self.pointee_of(base), Ok(Pointee::Float | Pointee::Double))
                    || matches!(base.as_ref(), Expression::Variable(name)
                        if self.global_array_sizes.contains_key(name.as_str())
                            && matches!(self.globals.get(name.as_str()), Some(Type::Float | Type::Double)))
            }
            Expression::Member { member_type, .. } => *member_type == Type::Float,
            // A cast TO a float type is a float value (`(double)x`); a cast to a
            // non-float type is not, regardless of the operand.
            Expression::Cast { target_type, .. } => matches!(target_type, Type::Float | Type::Double),
            _ => false,
        }
    }

    /// Emit a prefix unary operator into `destination`.
    pub(crate) fn emit_unary(&mut self, operator: UnaryOperator, operand: &Expression, destination: u8) -> Compilation<()> {
        let d = destination;
        match operator {
            UnaryOperator::Negate => {
                // Negating a literal folds to loading the negated constant.
                if let Expression::IntegerLiteral(value) = operand {
                    self.load_integer_constant(d, -*value);
                    return Ok(());
                }
                // -(-x) == x
                if let Expression::Unary { operator: UnaryOperator::Negate, operand: inner } = operand {
                    return self.evaluate_general(inner, d);
                }
                // -(x < 0) / -(x > 0): the sign-bit comparison idioms end in a logical
                // shift (`srwi 31`, giving 0/1); negating the boolean is just the
                // arithmetic shift (`srawi 31`, giving 0/-1) instead — no separate
                // `neg`, and (for `>`) the operand stays live for the `andc`.
                if let Expression::Binary { operator: comparison @ (BinaryOperator::Less | BinaryOperator::Greater), left, right } = operand {
                    if is_zero_literal(right) && self.signedness_of(left)? {
                        if *comparison == BinaryOperator::Less {
                            // -(x < 0) = srawi d, x, 31
                            let source = self.place_operand_or_scratch(left, d)?;
                            self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: d, s: source, shift: 31 });
                        } else {
                            // -(x > 0) = neg r0,x; andc r0,r0,x; srawi d,r0,31
                            self.evaluate_general(left, d)?;
                            self.output.instructions.push(Instruction::Negate { d: GENERAL_SCRATCH, a: d });
                            self.output.instructions.push(Instruction::AndComplement { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, b: d });
                            self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: d, s: GENERAL_SCRATCH, shift: 31 });
                        }
                        return Ok(());
                    }
                }
                let source = match self.signed_byte_scratch_source(operand, d)? {
                    Some(scratch) => scratch,
                    None => self.place_operand_or_scratch(operand, d)?,
                };
                self.output.instructions.push(Instruction::Negate { d, a: source });
            }
            UnaryOperator::BitNot => {
                // ~(~x) == x
                if let Expression::Unary { operator: UnaryOperator::BitNot, operand: inner } = operand {
                    return self.evaluate_general(inner, d);
                }
                // `~(a | b)` / `~(a & b)` / `~(a ^ b)` fuse to a single nor / nand /
                // eqv. Both operands must be in registers: two leaves, or a leaf and
                // a constant materialized into the scratch (`li r0,c; nor d,a,r0`).
                if let Expression::Binary { operator: inner @ (BinaryOperator::BitOr | BinaryOperator::BitAnd | BinaryOperator::BitXor), left, right } = operand {
                    let left_register = leaf_name(left).and_then(|name| self.lookup_general(name));
                    let right_register = leaf_name(right).and_then(|name| self.lookup_general(name));
                    let registers = match (left_register, right_register) {
                        (Some(ra), Some(rb)) => Some((ra, rb)),
                        (Some(ra), None) => constant_value(right).filter(|c| fits_signed_16(*c)).map(|constant| {
                            self.load_integer_constant(GENERAL_SCRATCH, constant);
                            (ra, GENERAL_SCRATCH)
                        }),
                        _ => None,
                    };
                    if let Some((ra, rb)) = registers {
                        self.output.instructions.push(match inner {
                            BinaryOperator::BitOr => Instruction::Nor { a: d, s: ra, b: rb },
                            BinaryOperator::BitAnd => Instruction::Nand { a: d, s: ra, b: rb },
                            _ => Instruction::Eqv { a: d, s: ra, b: rb },
                        });
                        return Ok(());
                    }
                }
                let source = match self.signed_byte_scratch_source(operand, d)? {
                    Some(scratch) => scratch,
                    None => self.place_operand_or_scratch(operand, d)?,
                };
                self.output.instructions.push(Instruction::Nor { a: d, s: source, b: source });
            }
            UnaryOperator::LogicalNot => {
                // Fold a chain of `!`: mwcc collapses repeated negations by parity.
                // An even chain is the boolean normalization `inner != 0`; an odd
                // chain is `inner == 0` — and `!comparison` flips the comparison.
                let mut negations = 1usize;
                let mut inner = operand;
                while let Expression::Unary { operator: UnaryOperator::LogicalNot, operand: next } = inner {
                    negations += 1;
                    inner = next;
                }
                let odd = negations % 2 == 1;
                // `!(comparison)` is the flipped comparison; an even chain keeps it.
                if let Expression::Binary { operator, left, right } = inner {
                    if is_comparison(*operator) {
                        let resolved = if odd { flip_comparison(*operator) } else { Some(*operator) };
                        if let Some(operator) = resolved {
                            return self.emit_comparison(operator, left, right, d);
                        }
                    }
                }
                // An even chain is `inner != 0` (the `neg; or; srwi` boolean).
                if !odd {
                    let zero = Expression::IntegerLiteral(0);
                    return self.emit_comparison(BinaryOperator::NotEqual, inner, &zero, d);
                }
                // An odd chain is `inner == 0`: cntlzw then srwi by 5.
                let source = match self.signed_byte_scratch_source(inner, d)? {
                    Some(scratch) => scratch,
                    None => self.place_operand_or_scratch(inner, d)?,
                };
                self.output.instructions.push(Instruction::CountLeadingZeros { a: GENERAL_SCRATCH, s: source });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: GENERAL_SCRATCH, shift: 5 });
            }
        }
        Ok(())
    }
}
