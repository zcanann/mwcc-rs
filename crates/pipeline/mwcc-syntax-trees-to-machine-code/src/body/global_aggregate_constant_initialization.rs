//! Reused-constant initialization of one large file-scope aggregate.
//!
//! Legacy MWCC keeps the repeated value and the absolute aggregate base live
//! across the complete store run. A scalar return occupies r3, so the base
//! migrates to r5 and the repeated value to r4 while the one exceptional value
//! serializes through r0. Per-statement lowering cannot recover that shared
//! lifetime after it has materialized each member address independently.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::DataSectionDisplacement;

struct GlobalAggregateConstantInitialization {
    aggregate: String,
    repeated: i16,
    exceptional: i16,
    returned: i16,
    offsets: Vec<i16>,
}

impl Generator {
    pub(crate) fn try_global_aggregate_constant_initialization(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = self.global_aggregate_constant_initialization(function) else {
            return Ok(false);
        };
        self.emit_global_aggregate_constant_initialization(&plan);
        Ok(true)
    }

    fn global_aggregate_constant_initialization(
        &self,
        function: &Function,
    ) -> Option<GlobalAggregateConstantInitialization> {
        if self.behavior.constant_store_schedule_style
            != mwcc_versions::ConstantStoreScheduleStyle::InterleavedPairs
            || self.behavior.global_addressing != GlobalAddressing::SmallData
            || !matches!(function.return_type, Type::Int | Type::UnsignedInt)
            || !function.parameters.is_empty()
            || !function.locals.is_empty()
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function.statements.len() < 3
        {
            return None;
        }

        let returned = i16::try_from(constant_value(function.return_expression.as_ref()?)?).ok()?;
        let mut aggregate = None;
        let mut offsets = Vec::with_capacity(function.statements.len());
        let mut values = Vec::with_capacity(function.statements.len());
        for statement in &function.statements {
            let Statement::Store {
                target:
                    Expression::Member {
                        base,
                        offset,
                        member_type,
                        index_stride: None,
                    },
                value,
            } = statement
            else {
                return None;
            };
            let Expression::Variable(name) = base.as_ref() else {
                return None;
            };
            if !matches!(
                member_type,
                Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. }
            ) || !matches!(
                self.globals.get(name),
                Some(Type::Struct { size, .. }) if *size > 8
            ) || !self.full_bss_globals.contains(name)
                || aggregate.as_ref().is_some_and(|known| known != name)
            {
                return None;
            }
            aggregate.get_or_insert_with(|| name.clone());
            offsets.push(i16::try_from(*offset).ok()?);
            values.push(i16::try_from(constant_value(value)?).ok()?);
        }

        // Measured legacy shape: the first value repeats for every store except
        // the second. Keeping this explicit avoids claiming unrelated constant
        // runs whose allocator window and issue order differ.
        let repeated = values[0];
        let exceptional = values[1];
        if repeated == exceptional || values[2..].iter().any(|value| *value != repeated) || {
            let mut unique_offsets = offsets.clone();
            unique_offsets.sort_unstable();
            unique_offsets.dedup();
            unique_offsets.len() != offsets.len()
        } {
            return None;
        }

        Some(GlobalAggregateConstantInitialization {
            aggregate: aggregate?,
            repeated,
            exceptional,
            returned,
            offsets,
        })
    }

    fn emit_global_aggregate_constant_initialization(
        &mut self,
        plan: &GlobalAggregateConstantInitialization,
    ) {
        const BSS_ANCHOR: &str = "...bss.0";
        self.output.pre_scheduled = true;
        self.emit_address_high(3, BSS_ANCHOR);
        self.record_relocation(RelocationKind::Addr16Lo, BSS_ANCHOR);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, plan.repeated));
        self.record_aggregate_displacement(&plan.aggregate);
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 5,
            offset: plan.offsets[0],
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, plan.exceptional));
        self.output
            .instructions
            .push(Instruction::load_immediate(3, plan.returned));
        self.record_aggregate_displacement(&plan.aggregate);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 5,
            offset: plan.offsets[1],
        });
        for &offset in &plan.offsets[2..] {
            self.record_aggregate_displacement(&plan.aggregate);
            self.output
                .instructions
                .push(Instruction::StoreWord { s: 4, a: 5, offset });
        }
        self.emit_epilogue_and_return();
    }

    fn record_aggregate_displacement(&mut self, aggregate: &str) {
        self.output
            .data_section_displacements
            .push(DataSectionDisplacement {
                instruction_index: self.output.instructions.len(),
                target: mwcc_machine_code::DataSectionDisplacementTarget::Symbol(
                    aggregate.to_owned(),
                ),
            });
    }
}
