//! Nominate later-target quotient/fill scheduling while volatility is available.
use crate::generator::Generator;
use mwcc_machine_code::{
    Instruction as I, LaterFillEntrySchedule, RelocationKind, RelocationTarget,
};
use mwcc_versions::{FrameConvention, MaterializationCopyStyle, Optimization, OptimizationGoal};
impl Generator {
    pub(crate) fn schedule_later_fill_entry(&mut self) {
        if self.behavior.frame_convention != FrameConvention::Predecrement
            || self.behavior.optimization < Optimization::O3
            || self.behavior.optimization_goal != OptimizationGoal::Performance
            || !self.behavior.scheduler_enabled
            || self.preceded_by_asm
        {
            return;
        }
        let Some(anchor) = &self.data_section_anchor else {
            return;
        };
        let Some(start) = self
            .output
            .instructions
            .iter()
            .position(|i| matches!(i, I::MoveToCountRegister { .. }))
            .and_then(|at| at.checked_sub(13))
        else {
            return;
        };
        let mut published_globals = Vec::new();
        for at in [start + 8, start + 10] {
            let Some(name) = self.output.relocations.iter().find_map(|r| {
                if r.instruction_index != at || r.kind != RelocationKind::EmbSda21 {
                    return None;
                }
                if let RelocationTarget::External(name) = &r.target {
                    Some(name)
                } else {
                    None
                }
            }) else {
                return;
            };
            if !self.globals.contains_key(name) || self.volatile_globals.contains(name) {
                return;
            }
            published_globals.push(name.clone());
        }
        if published_globals[0] == published_globals[1] {
            return;
        }
        self.output.later_fill_entry_schedule = Some(LaterFillEntrySchedule {
            scalar_void_calls: self
                .call_parameter_types
                .iter()
                .filter_map(|(name, parameters)| {
                    use mwcc_syntax_trees::Type;
                    (self.call_return_types.get(name) == Some(&Type::Void)
                        && !self.variadic_callees.contains(name)
                        && parameters.len() <= 8
                        && parameters.iter().all(|t| {
                            t.width() <= 32
                                && !matches!(t, Type::Float | Type::Void | Type::Struct { .. })
                        }))
                    .then(|| (name.clone(), parameters.len() as u8))
                })
                .collect(),
            anchor_symbol: anchor.anchor_symbol.clone(),
            published_globals: published_globals.try_into().unwrap(),
            add_immediate_copy: self.behavior.fixed_fill_cursor_copy_style
                == MaterializationCopyStyle::AddImmediateZero,
        });
    }
}
