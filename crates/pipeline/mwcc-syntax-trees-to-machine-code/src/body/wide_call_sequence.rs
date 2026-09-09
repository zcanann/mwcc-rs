//! Straight-line calls carrying opaque 64-bit values.
//!
//! Recognition builds value identities, independent of source names and local
//! reuse. Lifetime analysis assigns a pair of saved GPRs only when a value must
//! survive a call. The latest result can instead flow directly from r3:r4.
//! Arithmetic, escaping addresses, control flow and mixed-width arguments stay
//! with their existing owners until they have explicit pair operations.

use super::*;
use mwcc_versions::{Optimization, OptimizationGoal};
use std::collections::HashMap;

fn wide(ty: Type) -> bool {
    matches!(ty, Type::LongLong | Type::UnsignedLongLong)
}

#[derive(Debug)]
struct Call<'a> {
    name: &'a str,
    arguments: Vec<usize>,
    result: Option<usize>,
}

#[derive(Debug)]
struct Value {
    definition: usize,
    last_use: Option<usize>,
}

#[derive(Debug)]
struct Sequence<'a> {
    calls: Vec<Call<'a>>,
    values: Vec<Value>,
    returned: Option<usize>,
}

impl<'a> Sequence<'a> {
    fn call(
        &mut self,
        expression: &'a Expression,
        destination: Option<&str>,
        bindings: &mut HashMap<String, usize>,
        locals: &HashMap<&str, Type>,
        returns: &HashMap<String, Type>,
        parameters: &HashMap<String, Vec<Type>>,
    ) -> Option<()> {
        let Expression::Call { name, arguments } = expression else {
            return None;
        };
        let formals = parameters.get(name)?;
        if arguments.len() != formals.len()
            || arguments.len() > 4
            || formals.iter().any(|ty| !wide(*ty))
        {
            return None;
        }
        let mut inputs = Vec::new();
        for argument in arguments {
            let Expression::Variable(name) = argument else {
                return None;
            };
            let value = *bindings.get(name)?;
            self.values[value].last_use = Some(self.calls.len());
            inputs.push(value);
        }
        let result = if let Some(destination) = destination {
            if !wide(*locals.get(destination)?) || !wide(*returns.get(name)?) {
                return None;
            }
            let id = self.values.len();
            self.values.push(Value {
                definition: self.calls.len(),
                last_use: None,
            });
            bindings.insert(destination.into(), id);
            Some(id)
        } else {
            // Hidden aggregate results need a different call ABI.
            if matches!(returns.get(name), Some(Type::Struct { .. }) | None) {
                return None;
            }
            None
        };
        self.calls.push(Call {
            name,
            arguments: inputs,
            result,
        });
        Some(())
    }

    fn recognize(
        function: &'a Function,
        returns: &HashMap<String, Type>,
        parameters: &HashMap<String, Vec<Type>>,
    ) -> Option<Self> {
        if !function.parameters.is_empty()
            || !function.guards.is_empty()
            || !(function.return_type == Type::Void || wide(function.return_type))
            || function.locals.is_empty()
            || function.locals.iter().any(|l| {
                !wide(l.declared_type) || l.is_static || l.is_volatile || l.array_length.is_some()
            })
        {
            return None;
        }
        let locals = function
            .locals
            .iter()
            .map(|l| (l.name.as_str(), l.declared_type))
            .collect();
        let mut bindings = HashMap::new();
        let mut plan = Self {
            calls: Vec::new(),
            values: Vec::new(),
            returned: None,
        };
        for local in &function.locals {
            if let Some(value) = &local.initializer {
                plan.call(
                    value,
                    Some(&local.name),
                    &mut bindings,
                    &locals,
                    returns,
                    parameters,
                )?;
            }
        }
        let mut returned = function.return_expression.as_ref();
        for (index, statement) in function.statements.iter().enumerate() {
            match statement {
                Statement::Assign { name, value } => plan.call(
                    value,
                    Some(name),
                    &mut bindings,
                    &locals,
                    returns,
                    parameters,
                )?,
                Statement::Expression(Expression::Assign { target, value }) => {
                    let Expression::Variable(name) = target.as_ref() else {
                        return None;
                    };
                    plan.call(
                        value,
                        Some(name),
                        &mut bindings,
                        &locals,
                        returns,
                        parameters,
                    )?;
                }
                Statement::Expression(expression) => plan.call(
                    expression,
                    None,
                    &mut bindings,
                    &locals,
                    returns,
                    parameters,
                )?,
                Statement::Return(value)
                    if index + 1 == function.statements.len() && returned.is_none() =>
                {
                    returned = value.as_ref();
                }
                _ => return None,
            }
        }
        if wide(function.return_type) {
            let Expression::Variable(name) = returned? else {
                return None;
            };
            let id = *bindings.get(name)?;
            plan.values[id].last_use = Some(plan.calls.len());
            plan.returned = Some(id);
        } else if returned.is_some() {
            return None;
        }
        (!plan.values.is_empty()).then_some(plan)
    }

    /// Interval coloring in definition order. A call reads its inputs before
    /// defining its result, so the result may reuse a pair dying at that call.
    fn saved_pairs(&self, unoptimized: bool) -> Option<(Vec<Option<usize>>, usize)> {
        let mut ends = Vec::new();
        let mut homes = vec![None; self.values.len()];
        for (id, value) in self.values.iter().enumerate() {
            let Some(last_use) = value.last_use else {
                continue;
            };
            if !unoptimized && last_use <= value.definition + 1 {
                continue;
            }
            let home = ends
                .iter()
                .position(|end| *end <= value.definition)
                .unwrap_or(ends.len());
            if home == ends.len() {
                ends.push(last_use);
            } else {
                ends[home] = last_use;
            }
            homes[id] = Some(home);
        }
        // Keep this lane inside the measured individual/multiple-save frame
        // range. Larger pair pressure needs the helper-call frame policy.
        (ends.len() <= 2).then_some((homes, ends.len()))
    }
}

impl Generator {
    pub(crate) fn try_wide_call_sequence(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !self.output.instructions.is_empty() {
            return Ok(false);
        }
        let Some(plan) = Sequence::recognize(
            function,
            &self.call_return_types,
            &self.call_parameter_types,
        ) else {
            return Ok(false);
        };
        let unoptimized = self.behavior.optimization == Optimization::O0;
        let Some((homes, count)) = plan.saved_pairs(unoptimized) else {
            return Ok(false);
        };
        self.output.pre_scheduled = true;
        self.owns_link_register_schedule = true;
        let saved: Vec<u32> = (32 - count as u32 * 2..32).rev().collect();
        if self.behavior.frame_convention == FrameConvention::LinkageFirst {
            self.emit_linkage_first_nonleaf_prologue(&saved);
        } else {
            self.emit_plain_nonleaf_prologue();
            self.callee_saved = saved.clone();
            self.frame_size = ((8 + saved.len() as i16 * 4 + 15) & !15).max(16);
            // The plain prologue initially reserves its minimum frame.
            self.output.instructions[0] = Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -self.frame_size,
            };
            self.output.instructions[2] = Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: self.frame_size + 4,
            };
            for (index, &s) in saved.iter().enumerate() {
                self.output.instructions.push(Instruction::StoreWord {
                    s,
                    a: 1,
                    offset: self.frame_size - 4 * (index as i16 + 1),
                });
            }
        }
        let multiple = self.behavior.optimization_goal == OptimizationGoal::Size
            && self.behavior.frame_convention == FrameConvention::LinkageFirst
            && count != 0;
        if multiple {
            let start = self.output.instructions.len() - saved.len();
            self.output.instructions.truncate(start);
            self.output
                .instructions
                .push(Instruction::StoreMultipleWord {
                    s: *saved.last().unwrap(),
                    a: 1,
                    offset: self.frame_size - saved.len() as i16 * 4,
                });
        }
        let pair = |home: usize| {
            let top = 31 - home as u32 * 2;
            if unoptimized {
                (top - 1, top)
            } else {
                (top, top - 1)
            }
        };
        let mut current = None;
        for call in &plan.calls {
            // Highest ABI pair first: moving a fresh r3:r4 result into a later
            // argument must happen before an earlier argument overwrites it.
            let indices: Vec<_> = if unoptimized {
                (0..call.arguments.len()).collect()
            } else {
                (0..call.arguments.len()).rev().collect()
            };
            for index in indices {
                let value = call.arguments[index];
                let destination = 3 + index as u32 * 2;
                let source = if !unoptimized && current == Some(value) {
                    (3, 4)
                } else {
                    pair(homes[value].expect("live pair has a saved home"))
                };
                self.emit_wide_sequence_copy(destination + 1, source.1, unoptimized);
                self.emit_wide_sequence_copy(destination, source.0, unoptimized);
            }
            self.record_relocation(RelocationKind::Rel24, call.name);
            self.output.instructions.push(Instruction::BranchAndLink {
                target: call.name.into(),
            });
            current = call.result;
            if let Some(value) = current {
                if let Some(home) = homes[value] {
                    let destination = pair(home);
                    self.emit_wide_sequence_copy(destination.1, 4, unoptimized);
                    self.emit_wide_sequence_copy(destination.0, 3, unoptimized);
                }
            }
        }
        if let Some(value) = plan.returned {
            if unoptimized || current != Some(value) {
                let source = pair(homes[value].expect("returned pair has a saved home"));
                self.emit_wide_sequence_copy(4, source.1, unoptimized);
                self.emit_wide_sequence_copy(3, source.0, unoptimized);
            }
        }
        self.epilogue_lr_before_gprs =
            self.behavior.schedule_latency_slots && self.behavior.scheduler_enabled;
        let epilogue = self.output.instructions.len();
        self.emit_epilogue_and_return();
        // Legacy performance scheduling can publish LR before releasing SP.
        // Lower levels, scheduling-off and size mode retain the stack-first
        // order; Nintendo keeps that order even under performance scheduling.
        if self.behavior.frame_convention == FrameConvention::LinkageFirst
            && self.behavior.saved_gpr_epilogue_style
                != mwcc_versions::SavedGprEpilogueStyle::StackRestoreBeforeLinkRegisterReload
        {
            let link_first = self.epilogue_lr_before_gprs
                && self.behavior.optimization_goal == OptimizationGoal::Performance
                && !self.behavior.structured_saved_gpr_stack_first;
            let end = self.output.instructions.len();
            let emitted_link_first = matches!(
                self.output.instructions[end - 3],
                Instruction::MoveToLinkRegister { .. }
            );
            if emitted_link_first != link_first {
                self.output.instructions.swap(end - 3, end - 2);
            }
        }
        if multiple {
            let restores: Vec<_> = self.output.instructions[epilogue..]
                .iter()
                .enumerate()
                .filter_map(|(i, instruction)| {
                    matches!(
                        instruction,
                        Instruction::LoadWord {
                            d: 14..=31,
                            a: 1,
                            ..
                        }
                    )
                    .then_some(epilogue + i)
                })
                .collect();
            let first = restores[0];
            for &index in restores.iter().skip(1).rev() {
                self.output.instructions.remove(index);
            }
            self.output.instructions[first] = Instruction::LoadMultipleWord {
                d: *saved.last().unwrap(),
                a: 1,
                offset: self.frame_size - saved.len() as i16 * 4,
            };
        }
        Ok(true)
    }

    fn emit_wide_sequence_copy(&mut self, destination: u32, source: u32, unoptimized: bool) {
        if destination == source {
            return;
        }
        if unoptimized {
            self.output
                .instructions
                .push(Instruction::move_register(destination, source));
        } else {
            self.emit_callee_saved_home_copy(destination, source);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_definition_reuses_a_pair_only_after_its_last_read() {
        let plan = Sequence {
            calls: Vec::new(),
            values: vec![
                Value {
                    definition: 0,
                    last_use: Some(4),
                },
                Value {
                    definition: 1,
                    last_use: Some(3),
                },
                Value {
                    definition: 3,
                    last_use: Some(6),
                },
                Value {
                    definition: 4,
                    last_use: Some(7),
                },
                Value {
                    definition: 5,
                    last_use: Some(6),
                },
                Value {
                    definition: 7,
                    last_use: None,
                },
            ],
            returned: None,
        };
        assert_eq!(
            plan.saved_pairs(false),
            Some((vec![Some(0), Some(1), Some(1), Some(0), None, None], 2))
        );
        assert!(
            plan.saved_pairs(true).is_none(),
            "source homes exceed this lane's register budget"
        );
    }

    #[test]
    fn immediate_consumers_need_no_survivor_but_later_uses_do() {
        let plan = Sequence {
            calls: Vec::new(),
            values: vec![
                Value {
                    definition: 0,
                    last_use: Some(1),
                },
                Value {
                    definition: 1,
                    last_use: Some(3),
                },
            ],
            returned: None,
        };
        assert_eq!(plan.saved_pairs(false), Some((vec![None, Some(0)], 1)));
        assert_eq!(plan.saved_pairs(true), Some((vec![Some(0), Some(0)], 1)));
    }
}
