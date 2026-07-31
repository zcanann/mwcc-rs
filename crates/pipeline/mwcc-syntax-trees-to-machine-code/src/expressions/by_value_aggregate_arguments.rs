//! Emission of caller-owned objects for dereferenced aggregate arguments.
//!
//! Frame planning assigns every copy before body emission. This module verifies
//! the source call, copies all input objects before overwriting any argument
//! register with an outgoing address, and hands the positional address steps
//! back to the ordinary prototype-directed argument loop.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn prepare_structured_by_value_aggregate_call(
        &mut self,
        arguments: &[Expression],
        name: &str,
    ) -> Compilation<Option<StructuredByValueAggregateCall>> {
        let Some(plan) = self.structured_by_value_aggregate_plan.as_ref() else {
            return Ok(None);
        };
        let Some(call) = plan.calls.get(plan.next_call) else {
            return Ok(None);
        };
        if call.callee != name {
            return Ok(None);
        }
        if call.copies.iter().any(|copy| {
            !matches!(
                arguments.get(copy.argument_index),
                Some(Expression::Dereference { pointer })
                    if matches!(pointer.as_ref(), Expression::Variable(source) if source == &copy.source_pointer)
            )
        }) {
            return Err(Diagnostic::error(
                "structured by-value aggregate call no longer matches its frame plan",
            ));
        }
        let call = call.clone();

        let sources = call
            .copies
            .iter()
            .map(|copy| {
                self.lookup_general(&copy.source_pointer).ok_or_else(|| {
                    Diagnostic::error(format!(
                        "by-value aggregate source '{}' has no general-register home",
                        copy.source_pointer,
                    ))
                })
            })
            .collect::<Compilation<Vec<_>>>()?;
        for (copy, source) in call.copies.iter().zip(sources) {
            let paired = self.fresh_virtual_general_preferring(6);
            let mut source_offset = 0i16;
            while source_offset + 8
                <= i16::try_from(copy.size)
                    .map_err(|_| Diagnostic::error("by-value aggregate argument is too large"))?
            {
                self.output.instructions.push(Instruction::LoadWord {
                    d: paired,
                    a: source,
                    offset: source_offset,
                });
                self.output.instructions.push(Instruction::LoadWord {
                    d: GENERAL_SCRATCH,
                    a: source,
                    offset: source_offset + 4,
                });
                self.output.instructions.push(Instruction::StoreWord {
                    s: paired,
                    a: 1,
                    offset: copy.copy_offset + source_offset,
                });
                self.output.instructions.push(Instruction::StoreWord {
                    s: GENERAL_SCRATCH,
                    a: 1,
                    offset: copy.copy_offset + source_offset + 4,
                });
                source_offset += 8;
            }
            if source_offset < i16::try_from(copy.size).expect("size was checked above") {
                self.output.instructions.push(Instruction::LoadWord {
                    d: GENERAL_SCRATCH,
                    a: source,
                    offset: source_offset,
                });
                self.output.instructions.push(Instruction::StoreWord {
                    s: GENERAL_SCRATCH,
                    a: 1,
                    offset: copy.copy_offset + source_offset,
                });
            }
        }
        self.structured_by_value_aggregate_plan
            .as_mut()
            .expect("the plan was present above")
            .next_call += 1;
        Ok(Some(call))
    }
}
