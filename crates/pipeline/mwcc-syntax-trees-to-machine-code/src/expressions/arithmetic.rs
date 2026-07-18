//! Binary/additive/distributive integer arithmetic emitters.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Evaluate an integer expression into general register `destination`.
    /// mwcc collapses the bitwise distributive laws `(x&y)|(x&z) -> x&(y|z)`,
    /// `(x|y)&(x|z) -> x|(y&z)`, and `(x&y)^(x&z) -> x&(y^z)` to one inner op plus the
    /// outer, sharing the common factor `x`. When `x` is the first operand of the left
    /// inner node, rewrite to that form and evaluate it. A common factor in the second
    /// position (`(y&x)|(z&x)`) needs mwcc's value-first operand order, not yet modeled,
    /// so it defers rather than emit the longer un-distributed sequence.
    pub(crate) fn try_emit_distributive_bitwise(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        use BinaryOperator::*;
        let (
            Expression::Binary {
                operator: inner,
                left: la,
                right: lb,
            },
            Expression::Binary {
                operator: inner_right,
                left: ra,
                right: rb,
            },
        ) = (left, right)
        else {
            return Ok(false);
        };
        if inner != inner_right
            || !matches!(
                (operator, *inner),
                (BitOr, BitAnd) | (BitXor, BitAnd) | (BitAnd, BitOr)
            )
        {
            return Ok(false);
        }
        let other = if same_operand(la, ra) {
            rb
        } else if same_operand(la, rb) {
            ra
        } else if same_operand(lb, ra) || same_operand(lb, rb) {
            return Err(Diagnostic::error("a distributive bitwise factor in the second operand position needs mwcc's value-first order (roadmap)"));
        } else {
            return Ok(false);
        };
        let combined = Expression::Binary {
            operator,
            left: lb.clone(),
            right: other.clone(),
        };
        let rewritten = Expression::Binary {
            operator: *inner,
            left: la.clone(),
            right: Box::new(combined),
        };
        self.evaluate_general(&rewritten, destination)?;
        Ok(true)
    }

    /// mwcc reassociates a left-leaning pure-addition chain `(x + y) + z` into
    /// `x + (y + z)`: it evaluates the tail `y + z` into the destination first,
    /// then adds the leading operand `x`. `x` is copied to the scratch beforehand
    /// only when it lives in the destination (which the tail overwrites). Only a
    /// full-width integer leaf `x` with simple integer tail operands is taken;
    /// pointers (scaled arithmetic), narrow leaves, and deeper or right-leaning
    /// chains keep the generic paths.
    pub(crate) fn try_emit_additive_chain(
        &mut self,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        let Expression::Binary {
            operator: BinaryOperator::Add,
            left: x,
            right: y,
        } = left
        else {
            return Ok(false);
        };
        let (x, y, z) = (x.as_ref(), y.as_ref(), right);
        // `(loadA + loadB) + Z` reassociates in mwcc (`A + (B + Z)`) with an
        // allocator-specific register assignment we do not reproduce yet. Defer it
        // rather than fall through to the constant-fold path, which would emit the
        // left-associated form (a mismatch) now that the two-load add is selected.
        if self.is_word_load(x) && self.is_word_load(y) {
            return Err(Diagnostic::error(
                "additive chain over two loads needs the allocator (roadmap)",
            ));
        }
        let Some(x_register) = self.plain_integer_leaf_register(x) else {
            return Ok(false);
        };
        // The tail operands must be simple: a full-width integer leaf or a constant.
        let simple = |me: &Self, operand: &Expression| {
            constant_value(operand).is_some() || me.plain_integer_leaf_register(operand).is_some()
        };
        if !simple(self, y) || !simple(self, z) {
            return Ok(false);
        }
        // Saving x needs a scratch register distinct from the destination.
        if x_register == destination && destination == GENERAL_SCRATCH {
            return Ok(false);
        }
        let leading = if x_register == destination {
            self.output
                .instructions
                .push(Instruction::move_register(GENERAL_SCRATCH, x_register));
            GENERAL_SCRATCH
        } else {
            x_register
        };
        let tail = Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(y.clone()),
            right: Box::new(z.clone()),
        };
        self.evaluate_general(&tail, destination)?;
        self.output.instructions.push(Instruction::Add {
            d: destination,
            a: leading,
            b: destination,
        });
        Ok(true)
    }

    /// `loadA op loadB` where both operands are full-word memory loads from a
    /// COMMON base (`a[i] op a[j]`, `s->x op s->y`). mwcc's binary-node convention
    /// puts the secondary source in the scratch (r0) and the primary in a register
    /// the allocator colors; the shared base stays live, so the primary takes the
    /// next free volatile (r4). The primary is the left operand for `add` and the
    /// right operand for `subf` (which computes `b - a`), loaded first.
    /// `L op L` with both operands the identical side-effect-free value: mwcc
    /// uses it ONCE. `x&x`/`x|x` are the value itself; for a memory LOAD, `x+x`/
    /// `x*x`/`x<<x`/`x>>x` load once into the scratch then apply the op to that
    /// register twice (`add d,r0,r0`, `mullw`, `slw`, `sraw`/`srw`). `x-x`/`x^x`
    /// fold to 0 in `constant_value` before reaching here (kept as a fallback). A
    /// leaf `x+x`/`x*x`/… falls through — its operand is already in a register so
    /// the generic path emits `op d,a,a`.
    pub(crate) fn try_emit_identical_load_binary(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        use BinaryOperator::*;
        if !same_operand(left, right) {
            return Ok(false);
        }
        match operator {
            BitAnd | BitOr => self.evaluate_general(left, destination)?,
            Subtract | BitXor => self.load_integer_constant(destination, 0),
            Add | Multiply | ShiftLeft | ShiftRight if self.is_simple_word_load(left) => {
                self.evaluate_general(left, GENERAL_SCRATCH)?;
                let r = GENERAL_SCRATCH;
                let instruction = match operator {
                    Add => Instruction::Add {
                        d: destination,
                        a: r,
                        b: r,
                    },
                    Multiply => Instruction::MultiplyLow {
                        d: destination,
                        a: r,
                        b: r,
                    },
                    ShiftLeft => Instruction::ShiftLeftWord {
                        a: destination,
                        s: r,
                        b: r,
                    },
                    _ if self.signedness_of(left)? => Instruction::ShiftRightAlgebraicWord {
                        a: destination,
                        s: r,
                        b: r,
                    },
                    _ => Instruction::ShiftRightWord {
                        a: destination,
                        s: r,
                        b: r,
                    },
                };
                self.output.instructions.push(instruction);
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// `(x & m1) | (x & m2)` over the same leaf `x` is `x & (m1 | m2)` — mwcc
    /// merges the masks into one `rlwinm`/`clrlwi` rather than extracting both
    /// fields and OR-ing them.
    pub(crate) fn try_emit_same_leaf_mask_or(
        &mut self,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        let (Some((left_leaf, m1)), Some((right_leaf, m2))) =
            (as_masked_leaf(left), as_masked_leaf(right))
        else {
            return Ok(false);
        };
        if leaf_name(left_leaf).is_none() || leaf_name(left_leaf) != leaf_name(right_leaf) {
            return Ok(false);
        }
        let combined = Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left: Box::new(left_leaf.clone()),
            right: Box::new(Expression::IntegerLiteral((m1 | m2) as i64)),
        };
        self.evaluate_general(&combined, destination)?;
        Ok(true)
    }

    pub(crate) fn try_emit_two_load_binary(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        use BinaryOperator::*;
        if !matches!(
            operator,
            Add | Subtract | BitAnd | BitOr | BitXor | Multiply
        ) {
            return Ok(false);
        }
        // Single-instruction loads only: a variable-index subscript scales to
        // `slwi; lwzx`, and two of those mis-schedule against each other.
        if !self.is_simple_word_load(left) || !self.is_simple_word_load(right) {
            return Ok(false);
        }
        if load_base_name(left).is_none() || load_base_name(right).is_none() {
            return Ok(false);
        }
        // `subf` computes `b - a`; to get `left - right` the right operand is the
        // primary (first source) and the left is the secondary (in r0). The
        // commutative operators keep the left operand as the primary.
        let (primary, secondary) = match operator {
            Subtract => (right, left),
            _ => (left, right),
        };
        let primary_register = self.fresh_virtual_general();
        self.evaluate_general(primary, primary_register)?;
        self.evaluate_general(secondary, GENERAL_SCRATCH)?;
        let (p, s) = (primary_register, GENERAL_SCRATCH);
        let combined = match operator {
            Add => Instruction::Add {
                d: destination,
                a: p,
                b: s,
            },
            Subtract => Instruction::SubtractFrom {
                d: destination,
                a: p,
                b: s,
            },
            Multiply => Instruction::MultiplyLow {
                d: destination,
                a: p,
                b: s,
            },
            BitAnd => Instruction::And {
                a: destination,
                s: p,
                b: s,
            },
            BitOr => Instruction::Or {
                a: destination,
                s: p,
                b: s,
            },
            _ => Instruction::Xor {
                a: destination,
                s: p,
                b: s,
            },
        };
        self.output.instructions.push(combined);
        Ok(true)
    }

    /// `a[k] op x` — a constant-index subscript word-load combined with a wide
    /// integer leaf. The subscript loads into the scratch (`lwz r0,off(base)`) and
    /// the leaf stays in its register, like the dereference/member + leaf paths
    /// (subscripts just were not routed there). Source operand order is kept.
    pub(crate) fn try_emit_subscript_leaf_binary(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        use BinaryOperator::*;
        if !matches!(
            operator,
            Add | Subtract | BitAnd | BitOr | BitXor | Multiply
        ) {
            return Ok(false);
        }
        // A full-word subscript load with either a constant index (`a[3]`, a plain
        // `lwz off(base)`) or a variable index (`a[i]`, scaled `slwi r0,i,2; lwzx
        // r0,base,r0`). Either way `evaluate_general` lands it in the scratch, and
        // the binary then reads the wide leaf in place — matching mwcc's
        // `…; add d,leaf,r0`.
        let is_word_subscript = |me: &Self, expression: &Expression| {
            matches!(expression, Expression::Index { .. }) && me.is_word_load(expression)
        };
        // Exactly one operand is the subscript load; the other a wide integer leaf.
        let (load, leaf, load_is_left) = match (
            is_word_subscript(self, left),
            is_word_subscript(self, right),
        ) {
            (true, false) => (left, right, true),
            (false, true) => (right, left, false),
            _ => return Ok(false),
        };
        let Some(leaf_register) = self.plain_integer_leaf_register(leaf) else {
            return Ok(false);
        };
        // A scaled (variable-index) subscript is the heavier subtree, so mwcc
        // canonicalizes it to the SECOND operand of a commutative op (leaf first),
        // regardless of source order — `add d,leaf,r0`. A plain (constant-index)
        // subscript keeps source order. Subtract is non-commutative either way.
        let variable_index =
            matches!(load, Expression::Index { index, .. } if constant_value(index).is_none());
        let commutative = !matches!(operator, BinaryOperator::Subtract);
        self.evaluate_general(load, GENERAL_SCRATCH)?;
        let (a, b) = if commutative && variable_index {
            (leaf_register, GENERAL_SCRATCH)
        } else if load_is_left {
            (GENERAL_SCRATCH, leaf_register)
        } else {
            (leaf_register, GENERAL_SCRATCH)
        };
        let combined = match operator {
            Add => Instruction::Add {
                d: destination,
                a,
                b,
            },
            Subtract => Instruction::SubtractFrom {
                d: destination,
                a: b,
                b: a,
            },
            Multiply => Instruction::MultiplyLow {
                d: destination,
                a,
                b,
            },
            BitAnd => Instruction::And {
                a: destination,
                s: a,
                b,
            },
            BitOr => Instruction::Or {
                a: destination,
                s: a,
                b,
            },
            _ => Instruction::Xor {
                a: destination,
                s: a,
                b,
            },
        };
        self.output.instructions.push(combined);
        Ok(true)
    }

    /// `(cond ? c1 : c2) +/- k` with both arms constant distributes the constant
    /// into the arms — `cond ? (c1±k) : (c2±k)` — so the select's trailing `addi`
    /// absorbs it, as mwcc does (a leaf `(x?1:2)+5` is `…; addi r3,r3,7`).
    pub(crate) fn try_emit_select_constant_fold(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        let constant_select = |expression: &Expression| {
            matches!(expression,
            Expression::Conditional { when_true, when_false, .. }
                if constant_value(when_true).is_some() && constant_value(when_false).is_some())
        };
        // Add commutes (select either side); subtract distributes only `select - k`.
        let (select, delta) = match operator {
            BinaryOperator::Add if constant_select(left) => match constant_value(right) {
                Some(k) => (left, k),
                None => return Ok(false),
            },
            BinaryOperator::Add if constant_select(right) => match constant_value(left) {
                Some(k) => (right, k),
                None => return Ok(false),
            },
            BinaryOperator::Subtract if constant_select(left) => match constant_value(right) {
                Some(k) => (left, -k),
                None => return Ok(false),
            },
            _ => return Ok(false),
        };
        let Expression::Conditional {
            condition,
            when_true,
            when_false,
            origin,
        } = select
        else {
            return Ok(false);
        };
        if self.try_emit_constant_select_with_common_offset(
            condition,
            when_true,
            when_false,
            destination,
            delta,
            *origin,
        )? {
            return Ok(true);
        }
        let shifted_true = Expression::IntegerLiteral(constant_value(when_true).unwrap() + delta);
        let shifted_false = Expression::IntegerLiteral(constant_value(when_false).unwrap() + delta);
        self.emit_conditional(
            condition,
            &shifted_true,
            &shifted_false,
            destination,
            false,
            *origin,
        )?;
        Ok(true)
    }
}
