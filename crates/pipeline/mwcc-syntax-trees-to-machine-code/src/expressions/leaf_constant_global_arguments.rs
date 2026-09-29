//! Argument lists made of register leaves, small constants, and scalar globals.
//!
//! MWCC issues such an argument set in three groups, whatever their source
//! order (measured on GC/1.2.5n and GC/2.6):
//!
//! 1. incoming register values move to their argument registers first
//!    (`k3(g, 1, a)`: `mr r5,r3` before `g` claims r3);
//! 2. constants follow in argument order;
//! 3. global loads come last, in argument order (`k3(g, g2, 5)`:
//!    `li r5,5; lwz r3,g; lwz r4,g2`).
//!
//! The save scheduler then lifts the leading moves and constants into the
//! prologue's `mflr`->`stw r0` gap. A register permutation that needs a
//! temporary (a move cycle) is left to the dedicated permutation owners.

use super::*;

enum Group<'a> {
    Move { source: u32 },
    Constant(i16),
    Global(&'a str),
}

impl Generator {
    pub(crate) fn try_emit_leaf_constant_global_arguments(
        &mut self,
        arguments: &[Expression],
        name: &str,
        direct_call: bool,
    ) -> Compilation<bool> {
        let first = u32::from(Eabi::FIRST_GENERAL_ARGUMENT);
        if !direct_call || arguments.is_empty() || arguments.len() > 8 {
            return Ok(false);
        }
        // Prototype conversions (narrow parameters) change the emitted forms.
        if let Some(types) = self.call_parameter_types.get(name) {
            if types.len() != arguments.len()
                || !types.iter().all(|ty| {
                    matches!(
                        ty,
                        Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. }
                    )
                })
            {
                return Ok(false);
            }
        }
        let mut groups = Vec::with_capacity(arguments.len());
        for argument in arguments {
            let group = match argument {
                Expression::IntegerLiteral(value)
                    if (i64::from(i16::MIN)..=i64::from(i16::MAX)).contains(value) =>
                {
                    Group::Constant(*value as i16)
                }
                Expression::Variable(variable) if self.globals.contains_key(variable.as_str()) => {
                    if self.global_array_address_extent(variable).is_some()
                        || !matches!(
                            self.globals.get(variable.as_str()),
                            Some(
                                Type::Int
                                    | Type::UnsignedInt
                                    | Type::Short
                                    | Type::UnsignedShort
                                    | Type::Char
                                    | Type::UnsignedChar
                                    | Type::Pointer(_)
                                    | Type::StructPointer { .. }
                            )
                        )
                    {
                        return Ok(false);
                    }
                    Group::Global(variable.as_str())
                }
                Expression::Variable(variable) => {
                    match (self.lookup_general(variable), self.leaf_info(argument)) {
                        (Some(source), Ok((_, 32, _))) => Group::Move { source },
                        _ => return Ok(false),
                    }
                }
                _ => return Ok(false),
            };
            groups.push(group);
        }
        let has_global = groups.iter().any(|group| matches!(group, Group::Global(_)));
        let has_constant_after_global = groups
            .iter()
            .skip_while(|group| !matches!(group, Group::Global(_)))
            .any(|group| matches!(group, Group::Constant(_)));
        if !has_global || !has_constant_after_global {
            return Ok(false);
        }

        // Order the register moves as a parallel copy: a move may only run once
        // no pending move still reads its destination.
        let mut pending: Vec<(u32, u32)> = groups
            .iter()
            .enumerate()
            .filter_map(|(position, group)| match group {
                Group::Move { source } => Some((first + position as u32, *source)),
                _ => None,
            })
            .filter(|(destination, source)| destination != source)
            .collect();
        let mut ordered = Vec::with_capacity(pending.len());
        while !pending.is_empty() {
            let Some(index) = pending.iter().position(|(destination, _)| {
                !pending.iter().any(|(_, source)| source == destination)
            }) else {
                // A cycle needs a temporary; its placement is not modeled here.
                return Ok(false);
            };
            ordered.push(pending.remove(index));
        }
        for (destination, source) in ordered {
            self.output.instructions.push(Instruction::Or {
                a: destination,
                s: source,
                b: source,
            });
        }
        for (position, group) in groups.iter().enumerate() {
            if let Group::Constant(value) = group {
                self.output.instructions.push(Instruction::AddImmediate {
                    d: first + position as u32,
                    a: 0,
                    immediate: *value,
                });
            }
        }
        for (position, group) in groups.iter().enumerate() {
            if let Group::Global(global) = group {
                self.emit_global_load(global, first + position as u32)?;
            }
        }
        Ok(true)
    }
}
