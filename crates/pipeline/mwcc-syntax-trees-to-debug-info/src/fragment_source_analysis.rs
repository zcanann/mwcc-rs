//! Source-analysis ordinals in straight-line C debug units.
//!
//! The object writer's counter already includes named parameters from the whole
//! unit. Without file IPA, later parameters have not been analyzed when the
//! first line header is created. With IPA, later body scopes and declarations
//! precede that header too. These costs belong to debug analysis, independently
//! of register allocation and anonymous object payloads.

use mwcc_machine_code::{Instruction, MachineFunction};
use mwcc_syntax_trees::{Statement, TranslationUnit, Type};
use mwcc_versions::CompilerBuild;

#[derive(Default)]
pub(super) struct SourceAnalysis {
    pub local_costs: Vec<u32>,
    pub header_adjustment: i64,
}

impl SourceAnalysis {
    pub fn for_c_unit(
        unit: &TranslationUnit,
        machines: &[MachineFunction],
        build: CompilerBuild,
        ipa_file: bool,
    ) -> Option<Self> {
        let mut plan = Self::default();
        for (index, machine) in machines.iter().enumerate() {
            let function = unit
                .functions
                .iter()
                .find(|function| function.name == machine.name)?;
            // Control-flow analysis, aggregate images, and static-local scopes
            // have separate ordinal owners; retain their existing plans.
            if !function.guards.is_empty()
                || function.statements.iter().any(|statement| {
                    matches!(
                        statement,
                        Statement::If { .. } | Statement::Loop { .. } | Statement::Switch { .. }
                    )
                })
                || machine.instructions.iter().any(|instruction| {
                    matches!(
                        instruction,
                        Instruction::Branch { .. }
                            | Instruction::BranchConditionalForward { .. }
                            | Instruction::BranchConditionalToLinkRegister { .. }
                    )
                })
                || function.locals.iter().any(|local| {
                    local.is_static
                        || local.array_length.is_some()
                        || matches!(local.declared_type, Type::Struct { .. })
                        || local.data_bytes.is_some()
                })
            {
                return None;
            }
            let local_cost = function
                .locals
                .iter()
                .map(|local| {
                    if local.is_const {
                        u32::from(
                            build
                                .profile
                                .dropped_inline_const_local_declaration_label_weight(),
                        )
                    } else {
                        u32::from(
                            build
                                .profile
                                .dropped_inline_local_declaration_label_weight(),
                        )
                    }
                })
                .sum::<u32>();
            plan.local_costs.push(local_cost);
            if index != 0 {
                if ipa_file {
                    // Every analyzed body, including an assembly body, owns
                    // three frontend scope labels before its emitted code.
                    plan.header_adjustment += i64::from(3 + local_cost);
                } else {
                    plan.header_adjustment -=
                        i64::from(build.profile.dropped_inline_parameter_label_weight())
                            * function
                                .parameters
                                .iter()
                                .filter(|parameter| !parameter.name.is_empty())
                                .count() as i64;
                }
            }
        }
        Some(plan)
    }

    pub fn local_cost(&self, index: usize) -> u32 {
        self.local_costs.get(index).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_versions::{GC_3_0A3, WII_1_0};

    fn source(text: &str) -> (TranslationUnit, Vec<MachineFunction>) {
        let tokens = mwcc_source_to_tokens::tokenize(text).unwrap();
        let unit =
            mwcc_tokens_to_syntax_trees::parse_translation_unit(tokens, false, true, 3, 3).unwrap();
        let machines = unit
            .functions
            .iter()
            .map(|function| MachineFunction::new(&function.name))
            .collect();
        (unit, machines)
    }

    #[test]
    fn ipa_moves_later_analysis_before_header_without_moving_the_closing_scope() {
        let (unit, machines) =
            source("int first(void) {return 1;} int second(int x) {int y=2;return y;}");
        let ordinary = SourceAnalysis::for_c_unit(&unit, &machines, GC_3_0A3, false).unwrap();
        let ipa = SourceAnalysis::for_c_unit(&unit, &machines, GC_3_0A3, true).unwrap();
        assert_eq!(ordinary.local_costs, [0, 1]);
        assert_eq!(ordinary.header_adjustment, -1);
        assert_eq!(ipa.header_adjustment, 4);
        let ordinary = super::super::fragment_ordinals::fragment_ordinals_with_source(
            &machines, GC_3_0A3, 6, 3, &ordinary,
        )
        .unwrap();
        let ipa = super::super::fragment_ordinals::fragment_ordinals_with_source(
            &machines, GC_3_0A3, 6, 3, &ipa,
        )
        .unwrap();
        assert_eq!(ordinary, (5, 12));
        assert_eq!(ipa, (10, 12));
    }

    #[test]
    fn const_automatic_analysis_cost_follows_the_frontend_generation() {
        let (unit, machines) = source("int first(void) {const int x=2;return x;}");
        assert_eq!(
            SourceAnalysis::for_c_unit(&unit, &machines, GC_3_0A3, false)
                .unwrap()
                .local_costs,
            [2]
        );
        assert_eq!(
            SourceAnalysis::for_c_unit(&unit, &machines, WII_1_0, false)
                .unwrap()
                .local_costs,
            [1]
        );
    }

    #[test]
    fn gc247_charges_const_locals_but_not_mutable_locals_or_parameter_names() {
        let (unit, machines) = source(
            "int first(void) {int x=2;return x;} int second(int y) {const int z=3;return z;}",
        );
        let plan =
            SourceAnalysis::for_c_unit(&unit, &machines, mwcc_versions::GC_2_7, false).unwrap();
        assert_eq!(plan.local_costs, [0, 1]);
        assert_eq!(plan.header_adjustment, 0);
    }
}
