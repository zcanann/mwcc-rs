//! Constant fields share the word-RMW recognizer with parameterized fields.
//! This module schedules the proven update and its optional continuation.

use super::*;
use mwcc_versions::FixedAddressParameterizedRmwStyle as Style;

pub(super) struct ConstantRmw {
    pub address: u32,
    pub index: i64,
    pub mask: u16,
    pub set_bits: u16,
    pub reset: bool,
}

impl Generator {
    pub(super) fn try_emit_constant_fixed_word_rmw(
        &mut self,
        function: &Function,
        update: ConstantRmw,
        forward: Option<&Expression>,
    ) -> Compilation<bool> {
        if self.variadic_definition || !self.frame_slots.is_empty() {
            return Ok(false);
        }
        let status = match (&function.return_type, &function.return_expression) {
            (Type::Void, None) => None,
            (Type::Int | Type::UnsignedInt, Some(value)) => {
                let Some(value) = constant_value(value).and_then(|value| i16::try_from(value).ok())
                else {
                    return Ok(false);
                };
                Some(value)
            }
            _ => return Ok(false),
        };
        let call = if let Some(Expression::Call { name, arguments }) = forward {
            if status.is_some() || function.return_type != Type::Void
                || self.globals.contains_key(name) || self.locations.contains_key(name)
                || self.known_locals.contains(name) || self.variadic_callees.contains(name)
                || self.call_return_types.get(name) != Some(&Type::Void)
                || self.inline_bodies.asm_fragment(name).is_some()
                || self.inline_bodies.parameterized_asm_fragment(name).is_some()
                || crate::intrinsics::ordering_instruction(name, arguments.len()).is_some()
                || !self.call_parameter_types.get(name).is_some_and(|types| {
                    types.len() == arguments.len() && arguments.len() <= 8
                        && types.iter().zip(arguments).enumerate().all(|(index, (ty, argument))| {
                            matches!(ty, Type::Int | Type::UnsignedInt)
                                && matches!(argument, Expression::Variable(name) if self.locations.get(name).is_some_and(|location|
                                    location.class == ValueClass::General && location.width == 32 && location.register == index as u8 + 3))
                        })
                }) { return Ok(false); }
            Some((name, arguments))
        } else if forward.is_some() {
            return Ok(false);
        } else {
            None
        };
        let style = self.behavior.fixed_address_parameterized_rmw_style;
        let legacy = style == Style::Legacy233;
        let early_reset = style == Style::Early24 && update.reset;
        if early_reset && (status.is_none() || update.mask > i16::MAX as u16) {
            return Ok(false);
        }
        let (high, low) = crate::expressions::split_address(update.address);
        let folded = i16::try_from(i64::from(low) + update.index * 4)
            .map_err(|_| Diagnostic::error("constant fixed-register field is out of range"))?;
        let offset = if legacy {
            i16::try_from(update.index * 4)
                .map_err(|_| Diagnostic::error("constant fixed-register field is out of range"))?
        } else {
            folded
        };
        let tail = call.is_some() && self.behavior.terminal_indirect_tail_call;
        let framed = call.is_some() && !tail;
        let live: Vec<&Expression> = call
            .iter()
            .flat_map(|(_, arguments)| arguments.iter())
            .collect();
        let base = if early_reset {
            5
        } else if status.is_some() {
            4
        } else {
            self.free_register_avoiding(&live)?
        };
        let loaded = if early_reset { 4 } else { 0 };
        self.output.pre_scheduled = true;
        if framed {
            self.emit_plain_nonleaf_prologue();
        }
        let address_start = self.output.instructions.len();
        if legacy && status.is_some() {
            self.output.instructions.extend([
                Instruction::load_immediate_shifted(3, high),
                Instruction::LoadWord {
                    d: 0,
                    a: 3,
                    offset: folded,
                },
                Instruction::AddImmediate {
                    d: 4,
                    a: 3,
                    immediate: low,
                },
                Instruction::load_immediate(3, status.unwrap()),
            ]);
        } else {
            self.output
                .instructions
                .push(Instruction::load_immediate_shifted(base, high));
            if legacy {
                self.output.instructions.push(Instruction::AddImmediate {
                    d: base,
                    a: base,
                    immediate: low,
                });
            }
            if framed {
                let high = self.output.instructions.remove(address_start);
                let linkage = self.behavior.frame_convention == FrameConvention::LinkageFirst;
                self.output
                    .instructions
                    .insert(if linkage { 1 } else { 2 }, high);
                if linkage && legacy {
                    let low = self.output.instructions.remove(address_start + 1);
                    self.output.instructions.insert(address_start, low);
                }
            }
            if early_reset {
                self.output
                    .instructions
                    .push(Instruction::load_immediate(0, update.mask as i16));
            } else if let Some(status) = status {
                self.output
                    .instructions
                    .push(Instruction::load_immediate(3, status));
            }
            self.output.instructions.push(Instruction::LoadWord {
                d: loaded,
                a: base,
                offset,
            });
            if early_reset {
                self.output
                    .instructions
                    .push(Instruction::load_immediate(3, status.unwrap()));
            }
        }
        self.output.instructions.extend([
            Instruction::AndImmediateRecord {
                a: loaded,
                s: loaded,
                immediate: update.mask,
            },
            Instruction::OrImmediate {
                a: loaded,
                s: loaded,
                immediate: update.set_bits,
            },
            Instruction::StoreWord {
                s: loaded,
                a: base,
                offset,
            },
        ]);
        if update.reset {
            self.output.instructions.extend([
                Instruction::LoadWord {
                    d: loaded,
                    a: base,
                    offset,
                },
                if early_reset {
                    Instruction::And {
                        a: 0,
                        s: loaded,
                        b: 0,
                    }
                } else {
                    Instruction::AndImmediateRecord {
                        a: 0,
                        s: loaded,
                        immediate: update.mask,
                    }
                },
                Instruction::StoreWord {
                    s: 0,
                    a: base,
                    offset,
                },
            ]);
        }
        if let Some((name, arguments)) = call {
            if tail {
                self.emit_direct_sibling_call(name, arguments)?;
                return Ok(true);
            }
            self.emit_call(name, arguments, None, false)?;
            if self.behavior.frame_convention == FrameConvention::LinkageFirst
                && self.behavior.plain_linkage_epilogue_style
                    != PlainLinkageEpilogueStyle::StackRestoreBeforeReload
            {
                self.output.instructions.extend([
                    Instruction::LoadWord {
                        d: 0,
                        a: 1,
                        offset: self.frame_size + 4,
                    },
                    Instruction::AddImmediate {
                        d: 1,
                        a: 1,
                        immediate: self.frame_size,
                    },
                    Instruction::MoveToLinkRegister { s: 0 },
                    Instruction::BranchToLinkRegister,
                ]);
                return Ok(true);
            }
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
