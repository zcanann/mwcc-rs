//! Legacy packet snapshots: retain the tested word through its publications.

use super::super::*;
use crate::packet_publication::{self, Publication};
use mwcc_versions::{Optimization, OptimizationGoal, PacketPublicationStyle as Style};

impl Generator {
    pub(crate) fn try_packet_publication(&mut self, function: &Function) -> Compilation<bool> {
        let style = self.behavior.packet_publication_style;
        if self.behavior.global_addressing != mwcc_versions::GlobalAddressing::SmallData
            || style == Style::Structured
            || self.behavior.optimization != Optimization::O4
            || self.behavior.optimization_goal != OptimizationGoal::Performance
            || !self.behavior.scheduler_enabled
            || self.behavior.power_pc_7400_scheduling_enabled()
            || function.peephole_disabled
            || self.variadic_definition
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let query = packet_publication::query(function);
        let helper = packet_publication::helper(function);
        let Some(plan) = query.as_ref().map(|q| &q.publication).or(helper.as_ref()) else {
            return Ok(false);
        };
        if [plan.whole, plan.field]
            .iter()
            .any(|name| !matches!(self.globals.get(*name), Some(Type::Int | Type::UnsignedInt)))
            || self.globals.get(plan.flag) != Some(&Type::UnsignedChar)
            || [plan.whole, plan.field, plan.flag].iter().any(|name| {
                self.volatile_globals.contains(*name)
                    || self.global_arrays.contains(*name)
                    || self.locations.contains_key(*name)
                    || self.full_bss_globals.contains(*name)
            })
            || ![plan.status, plan.read].iter().all(|name| {
                self.packet_call_abi_is_compatible(
                    name,
                    &[Type::Pointer(Pointee::UnsignedInt)],
                    false,
                )
            })
            || query.as_ref().is_some_and(|q| {
                !self.packet_call_abi_is_compatible(q.acquire, &[], true)
                    || !self.packet_call_abi_is_compatible(q.release, &[Type::Int], false)
                    || [plan.whole, plan.field, plan.flag].contains(&q.token)
            })
        {
            return Ok(false);
        }
        let Some(ready) = rlwinm_mask(i64::from(plan.ready)) else {
            return Ok(false);
        };
        let Some(preserve) = rlwinm_mask(i64::from(plan.preserve)) else {
            return Ok(false);
        };
        let Some(tag) = rlwinm_mask(i64::from(plan.tag_mask)) else {
            return Ok(false);
        };
        let Some(field) = rlwinm_mask(i64::from(plan.field_mask)) else {
            return Ok(false);
        };
        if plan.tag & 0xffff != 0 {
            return Ok(false);
        }
        self.non_leaf = true;
        self.output.pre_scheduled = true;
        self.frame_size = if query.is_some() { 24 } else { 16 };
        if query.is_some() {
            self.callee_saved = vec![31];
        }
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        if query.is_some() {
            self.output
                .instructions
                .push(Instruction::load_immediate(3, 0));
        }
        self.output.instructions.extend([
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -(self.frame_size as i16),
            },
        ]);
        let mut exits = Vec::new();
        if let Some(q) = &query {
            self.output.instructions.push(Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 20,
            });
            self.packet_load(q.publication.field, 0);
            self.packet_store(q.publication.flag, 3, true);
            self.output
                .instructions
                .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
            exits.push(self.packet_exit(4));
            self.packet_call_instruction(q.acquire);
            self.output.instructions.push(Instruction::AddImmediate {
                d: if style == Style::LegacyPatched { 0 } else { 31 },
                a: 3,
                immediate: 0,
            });
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 8,
        });
        if query.is_some() && style == Style::LegacyPatched {
            self.output
                .instructions
                .push(Instruction::move_register(31, 0));
        }
        self.packet_call_instruction(plan.status);
        let slot = 8 + plan.index as i16 * 4;
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: slot,
            },
            Instruction::AndMaskRecord {
                a: 0,
                s: 0,
                begin: ready.0,
                end: ready.1,
            },
        ]);
        exits.push(self.packet_exit(12));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 8,
        });
        self.packet_call_instruction(plan.read);
        self.emit_packet_fields(plan, slot, preserve, tag, field, &mut exits);
        let end = self.output.instructions.len();
        for index in exits {
            if let Instruction::BranchConditionalForward { target, .. } =
                &mut self.output.instructions[index]
            {
                *target = end;
            }
        }
        if let Some(q) = &query {
            self.output
                .instructions
                .push(Instruction::move_register(3, 31));
            self.packet_call_instruction(q.release);
            if style != Style::LegacyLateResult {
                self.packet_load(plan.field, 3);
            }
        }
        if style != Style::LegacyPatched {
            self.output.instructions.push(Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: self.frame_size as i16 + 4,
            });
        }
        if query.is_some() {
            self.output.instructions.push(Instruction::LoadWord {
                d: 31,
                a: 1,
                offset: 20,
            });
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: self.frame_size as i16,
        });
        if style == Style::LegacyPatched {
            self.output.instructions.push(Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 4,
            });
        }
        if query.is_some() && style == Style::LegacyLateResult {
            self.packet_load(plan.field, 3);
        }
        self.output.instructions.extend([
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }

    fn packet_call_abi_is_compatible(
        &self,
        name: &str,
        arguments: &[Type],
        result_word: bool,
    ) -> bool {
        !self.globals.contains_key(name)
            && !self.locations.contains_key(name)
            && !self.known_locals.contains(name)
            && !self.variadic_callees.contains(name)
            && self.inline_bodies.asm_fragment(name).is_none()
            && self
                .inline_bodies
                .parameterized_asm_fragment(name)
                .is_none()
            && crate::intrinsics::ordering_instruction(name, arguments.len()).is_none()
            && self.call_return_types.get(name).is_some_and(|ty| {
                matches!(ty, Type::Int | Type::UnsignedInt) || (!result_word && *ty == Type::Void)
            })
            && self.call_parameter_types.get(name).is_some_and(|types| {
                types.len() == arguments.len()
                    && types
                        .iter()
                        .zip(arguments)
                        .all(|(actual, expected)| match expected {
                            Type::Pointer(_) => matches!(actual, Type::Pointer(_)),
                            Type::Int => matches!(actual, Type::Int | Type::UnsignedInt),
                            _ => actual == expected,
                        })
            })
    }

    fn emit_packet_fields(
        &mut self,
        plan: &Publication<'_>,
        slot: i16,
        preserve: (u8, u8),
        tag: (u8, u8),
        field: (u8, u8),
        exits: &mut Vec<usize>,
    ) {
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: slot,
            },
            Instruction::AndContiguousMask {
                a: 0,
                s: 0,
                begin: preserve.0,
                end: preserve.1,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: slot,
            },
            Instruction::LoadWord {
                d: 4,
                a: 1,
                offset: slot,
            },
            Instruction::AndContiguousMask {
                a: 3,
                s: 4,
                begin: tag.0,
                end: tag.1,
            },
            Instruction::AddImmediateShifted {
                d: 0,
                a: 3,
                immediate: (0u32.wrapping_sub(plan.tag) >> 16) as i16,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
        ]);
        exits.push(self.packet_exit(4));
        self.output
            .instructions
            .push(Instruction::AndContiguousMask {
                a: 3,
                s: 4,
                begin: field.0,
                end: field.1,
            });
        self.packet_store(plan.whole, 4, false);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, i16::from(plan.flag_value)));
        self.packet_store(plan.field, 3, false);
        self.packet_store(plan.flag, 0, true);
    }

    fn packet_exit(&mut self, options: u8) -> usize {
        let index = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options,
                condition_bit: 2,
                target: 0,
            });
        index
    }
    fn packet_call_instruction(&mut self, name: &str) {
        self.record_relocation(RelocationKind::Rel24, name);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: name.to_owned(),
        });
    }
    fn packet_load(&mut self, name: &str, d: u8) {
        self.record_relocation(RelocationKind::EmbSda21, name);
        self.output
            .instructions
            .push(Instruction::LoadWord { d, a: 0, offset: 0 });
    }
    fn packet_store(&mut self, name: &str, s: u8, byte: bool) {
        self.record_relocation(RelocationKind::EmbSda21, name);
        self.output.instructions.push(if byte {
            Instruction::StoreByte { s, a: 0, offset: 0 }
        } else {
            Instruction::StoreWord { s, a: 0, offset: 0 }
        });
    }
}
