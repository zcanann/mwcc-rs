//! One-time initialization of a pair of mirrored global pointers.
//!
//! The two call results do not need callee-saved registers: mwcc commits each
//! result to its backing global, then reloads the first while the second remains
//! in the result register. This module owns that cross-call/store schedule.

#[allow(unused_imports)]
use super::*;

struct GuardedPointerPairInitialization<'a> {
    initialized: &'a str,
    first: &'a str,
    second: &'a str,
    first_mirror: &'a str,
    second_mirror: &'a str,
    first_call: &'a str,
    second_call: &'a str,
}

impl Generator {
    pub(crate) fn try_guarded_pointer_pair_initialization(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = recognize(self, function) else {
            return Ok(false);
        };

        self.emit_plain_nonleaf_prologue();
        self.emit_global_load(shape.initialized, 0)?;
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        let epilogue = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, epilogue);

        self.record_relocation(RelocationKind::Rel24, shape.first_call);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.first_call.to_string(),
        });
        self.emit_global_store(shape.first, Pointee::UnsignedInt, 3)?;

        self.record_relocation(RelocationKind::Rel24, shape.second_call);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.second_call.to_string(),
        });
        self.emit_global_load(shape.first, 4)?;
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.emit_global_store(shape.second, Pointee::UnsignedInt, 3)?;
        self.emit_global_store(shape.first_mirror, Pointee::UnsignedInt, 4)?;
        self.emit_global_store(shape.second_mirror, Pointee::UnsignedInt, 3)?;
        self.emit_global_store(shape.initialized, Pointee::UnsignedChar, 0)?;

        self.bind_label(epilogue);
        self.emit_epilogue_and_return();
        Ok(true)
    }
}

fn recognize<'a>(
    generator: &Generator,
    function: &'a Function,
) -> Option<GuardedPointerPairInitialization<'a>> {
    if generator.behavior.global_addressing != GlobalAddressing::SmallData
        || generator.behavior.frame_convention != FrameConvention::Predecrement
        || !generator.frame_slots.is_empty()
        || !function.parameters.is_empty()
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || function.return_type != Type::Void
        || function.return_expression.is_some()
    {
        return None;
    }
    let [Statement::If {
        condition: Expression::Variable(initialized),
        then_body,
        else_body,
    }, first_store, second_store, first_mirror_store, second_mirror_store, initialized_store] =
        function.statements.as_slice()
    else {
        return None;
    };
    if !matches!(then_body.as_slice(), [Statement::Return(None)]) || !else_body.is_empty() {
        return None;
    }
    let (first, first_call) = call_store(first_store)?;
    let (second, second_call) = call_store(second_store)?;
    let (first_mirror, first_source) = copy_store(first_mirror_store)?;
    let (second_mirror, second_source) = copy_store(second_mirror_store)?;
    if first_source != first
        || second_source != second
        || constant_store(initialized_store, 1)? != initialized
        || generator.globals.get(initialized) != Some(&Type::UnsignedChar)
    {
        return None;
    }
    let pointer_global = |name: &str| {
        matches!(
            generator.globals.get(name),
            Some(Type::Pointer(_) | Type::StructPointer { .. })
        )
    };
    if ![first, second, first_mirror, second_mirror]
        .into_iter()
        .all(pointer_global)
    {
        return None;
    }
    let mut globals = [initialized.as_str(), first, second, first_mirror, second_mirror];
    globals.sort_unstable();
    if globals.windows(2).any(|pair| pair[0] == pair[1]) {
        return None;
    }

    Some(GuardedPointerPairInitialization {
        initialized,
        first,
        second,
        first_mirror,
        second_mirror,
        first_call,
        second_call,
    })
}

fn call_store(statement: &Statement) -> Option<(&str, &str)> {
    let Statement::Store {
        target: Expression::Variable(target),
        value: Expression::Call { name, arguments },
    } = statement
    else {
        return None;
    };
    arguments.is_empty().then_some((target, name))
}

fn copy_store(statement: &Statement) -> Option<(&str, &str)> {
    let Statement::Store {
        target: Expression::Variable(target),
        value: Expression::Variable(source),
    } = statement
    else {
        return None;
    };
    Some((target, source))
}

fn constant_store(statement: &Statement, expected: i64) -> Option<&str> {
    let Statement::Store {
        target: Expression::Variable(target),
        value,
    } = statement
    else {
        return None;
    };
    (constant_value(value) == Some(expected)).then_some(target)
}
