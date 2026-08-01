//! Sparse structured switches emitted as balanced comparison trees.
//!
//! Liveness uses the canonical nested-if view, while emission retains small
//! sparse switches so it can choose MWCC's comparison pivots directly. This
//! also keeps source-shared fallthrough bodies unique: cloning a run of empty
//! case labels duplicates callback transactions and prevents the balanced tree
//! from forming.

use super::structured_dense_switch::statements_fall_through;
use super::structured_entry_alias::EntryParameterAlias;
#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::ArmBody;

#[derive(Clone, Copy)]
struct CaseRange {
    low: i64,
    high: i64,
    body: usize,
}

pub(super) fn is_sparse_retained_switch(arms: &[mwcc_syntax_trees::SwitchArm]) -> bool {
    let has_fallthrough = arms.iter().any(|arm| arm.falls_through);
    if arms.is_empty()
        || (!has_fallthrough
            && super::structured_switch_lowering::is_dense_structured_switch(arms))
        || arms
            .iter()
            .any(|arm| !(i16::MIN as i64..i16::MAX as i64).contains(&arm.value))
    {
        return false;
    }
    let mut values = std::collections::HashSet::with_capacity(arms.len());
    if !arms.iter().all(|arm| values.insert(arm.value)) {
        return false;
    }
    let ranges = shared_case_ranges(arms);
    let prefer_lower_root = !has_fallthrough && !case_ranges_are_contiguous(&ranges);
    ranges.len() <= 6
        && dispatch_range_is_supported(
            &ranges,
            0,
            ranges.len() - 1,
            None,
            prefer_lower_root,
            has_fallthrough,
        )
}

pub(super) fn has_direct_call_sparse_switch(function: &Function) -> bool {
    function.statements.iter().any(|statement| {
        matches!(statement,
            Statement::Switch {
                scrutinee: Expression::Call { .. },
                arms,
                ..
            } if is_sparse_retained_switch(arms))
    })
}

impl Generator {
    /// Build 163 treats the saved owner forwarded by a sparse-switch tail call
    /// as a value materialization. The ordinary pointer argument path retains
    /// `mr`; rewrite only the fully proven `(saved owner, constant, switch
    /// result)` terminal call window owned by this lowering.
    pub(super) fn schedule_sparse_switch_tail_argument_copy(&mut self, source: u8) {
        if self.behavior.materialization_copy_style
            != mwcc_versions::MaterializationCopyStyle::AddImmediateZero
        {
            return;
        }
        rewrite_sparse_switch_tail_argument_copy(&mut self.output.instructions, source);
    }
}

fn rewrite_sparse_switch_tail_argument_copy(
    instructions: &mut [Instruction],
    source: u8,
) -> bool {
    let Some(copy_index) = (0..instructions.len().saturating_sub(3)).rev().find(|index| {
        matches!(
            instructions[*index],
            Instruction::Or { a: 3, s, b } if s == source && b == source
        ) && matches!(
            instructions[*index + 1],
            Instruction::AddImmediate { d: 4, a: 0, .. }
        ) && matches!(
            instructions[*index + 2],
            Instruction::Or { a: 5, s, b } if s == b
        ) && matches!(
            instructions[*index + 3],
            Instruction::BranchAndLink { .. }
        )
    }) else {
        return false;
    };
    instructions[copy_index] = Instruction::AddImmediate {
        d: 3,
        a: source,
        immediate: 0,
    };
    true
}

fn shared_case_ranges(arms: &[mwcc_syntax_trees::SwitchArm]) -> Vec<CaseRange> {
    let mut source_targets = (0..arms.len()).collect::<Vec<_>>();
    for source_index in (0..arms.len()).rev() {
        let empty_fallthrough = matches!(
            &arms[source_index].body,
            ArmBody::Statements(statements) if statements.is_empty()
        ) && arms[source_index].falls_through;
        if empty_fallthrough && source_index + 1 < arms.len() {
            source_targets[source_index] = source_targets[source_index + 1];
        }
    }

    let sorted_index_by_value = {
        let mut values = arms
            .iter()
            .enumerate()
            .map(|(source_index, arm)| (arm.value, source_index))
            .collect::<Vec<_>>();
        values.sort_by_key(|(value, _)| *value);
        values
            .iter()
            .enumerate()
            .map(|(sorted_index, (value, _))| (*value, sorted_index))
            .collect::<std::collections::HashMap<_, _>>()
    };
    let mut cases = arms
        .iter()
        .enumerate()
        .map(|(source_index, arm)| {
            (
                arm.value,
                sorted_index_by_value[&arms[source_targets[source_index]].value],
            )
        })
        .collect::<Vec<_>>();
    cases.sort_by_key(|(value, _)| *value);

    let mut ranges = Vec::<CaseRange>::new();
    for (value, body) in cases {
        if let Some(previous) = ranges.last_mut() {
            if previous.high.checked_add(1) == Some(value) && previous.body == body {
                previous.high = value;
                continue;
            }
        }
        ranges.push(CaseRange {
            low: value,
            high: value,
            body,
        });
    }
    ranges
}

fn case_ranges_are_contiguous(ranges: &[CaseRange]) -> bool {
    ranges.windows(2).all(|pair| {
        pair[0].high.checked_add(1) == Some(pair[1].low)
    })
}

fn dispatch_range_is_supported(
    ranges: &[CaseRange],
    lo: usize,
    hi: usize,
    upper_bound: Option<i64>,
    prefer_lower_root: bool,
    prefer_upper_pair: bool,
) -> bool {
    lo == hi
        || shared_case_pivot(
            ranges,
            lo,
            hi,
            upper_bound,
            prefer_lower_root,
            prefer_upper_pair,
        )
        .is_some()
}

fn shared_case_pivot(
    ranges: &[CaseRange],
    lo: usize,
    hi: usize,
    upper_bound: Option<i64>,
    prefer_lower_root: bool,
    prefer_upper_pair: bool,
) -> Option<usize> {
    let count = hi - lo + 1;
    let preferred = if prefer_upper_pair && count == 2 && upper_bound.is_some() {
        hi
    } else if upper_bound.is_none() && !prefer_lower_root {
        lo + count / 2
    } else {
        lo + (count - 1) / 2
    };
    (lo..=hi)
        .map(|index| (index.abs_diff(preferred), usize::from(index > preferred), index))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|(_, _, index)| index)
        .find(|index| {
            let pivot = ranges[*index];
            pivot.low == pivot.high
                && (*index == lo
                    || dispatch_range_is_supported(
                        ranges,
                        lo,
                        *index - 1,
                        Some(pivot.low - 1),
                        false,
                        prefer_upper_pair,
                    ))
                && (*index == hi
                    || dispatch_range_is_supported(
                        ranges,
                        *index + 1,
                        hi,
                        upper_bound,
                        false,
                        prefer_upper_pair,
                    ))
        })
}

impl Generator {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit_structured_sparse_switch(
        &mut self,
        scrutinee: &Expression,
        arms: &[mwcc_syntax_trees::SwitchArm],
        default: Option<&ArmBody>,
        function: &Function,
        ephemeral_locals: &[&LocalDeclaration],
        return_branches: &mut Vec<usize>,
        label_positions: &mut std::collections::HashMap<String, usize>,
        pending_gotos: &mut Vec<(usize, String)>,
        entry_alias: &mut Option<EntryParameterAlias>,
    ) -> Compilation<()> {
        if !is_sparse_retained_switch(arms) {
            return Err(Diagnostic::error(
                "structured switch was retained without a sparse shared-body plan",
            ));
        }

        let register = match scrutinee {
            Expression::Variable(name) if self.locations.contains_key(name) => {
                let location = &self.locations[name];
                if location.class != ValueClass::General {
                    return Err(Diagnostic::error(
                        "structured switch scrutinee is not an integer",
                    ));
                }
                location.register
            }
            Expression::Call { .. } => {
                self.evaluate_general(scrutinee, Eabi::FIRST_GENERAL_ARGUMENT)?;
                Eabi::FIRST_GENERAL_ARGUMENT
            }
            _ => {
                self.evaluate_general(scrutinee, GENERAL_SCRATCH)?;
                GENERAL_SCRATCH
            }
        };

        let ranges = shared_case_ranges(arms);
        let prefer_lower_root = !arms.iter().any(|arm| arm.falls_through)
            && !case_ranges_are_contiguous(&ranges);
        let mut dispatch_patches = Vec::new();
        self.lower_shared_case_range(
            register,
            &ranges,
            0,
            ranges.len() - 1,
            None,
            None,
            prefer_lower_root,
            arms.iter().any(|arm| arm.falls_through),
            &mut dispatch_patches,
        );

        let mut sorted = arms.iter().collect::<Vec<_>>();
        sorted.sort_by_key(|arm| arm.value);
        let sorted_index_by_value = sorted
            .iter()
            .enumerate()
            .map(|(index, arm)| (arm.value, index))
            .collect::<std::collections::HashMap<_, _>>();
        let mut body_start = vec![0usize; arms.len()];
        let mut join_branches = Vec::new();
        for (source_index, arm) in arms.iter().enumerate() {
            body_start[sorted_index_by_value[&arm.value]] = self.output.instructions.len();
            self.reset_structured_switch_edge_caches();
            let falls_through_body = match &arm.body {
                ArmBody::Statements(statements) => {
                    self.emit_structured_arm_with_global_pointer_cache(
                        statements,
                        function,
                        ephemeral_locals,
                        return_branches,
                        label_positions,
                        pending_gotos,
                        entry_alias,
                    )?;
                    statements_fall_through(statements)
                }
                ArmBody::Return(value) => {
                    let result = match function.return_type {
                        Type::Float | Type::Double => Eabi::float_result().number,
                        _ => Eabi::general_result().number,
                    };
                    self.evaluate(value, function.return_type, result)?;
                    return_branches.push(self.output.instructions.len());
                    self.output.instructions.push(Instruction::Branch {
                        target: super::structured_early_return_schedule::
                            STRUCTURED_EPILOGUE_PLACEHOLDER,
                    });
                    false
                }
            };
            if falls_through_body
                && !arm.falls_through
                && (source_index + 1 != arms.len() || default.is_some())
            {
                join_branches.push(self.output.instructions.len());
                self.output
                    .instructions
                    .push(Instruction::Branch { target: 0 });
            }
        }

        let default_start = self.output.instructions.len();
        if let Some(default) = default {
            self.reset_structured_switch_edge_caches();
            match default {
                ArmBody::Statements(statements) => {
                    self.emit_structured_arm_with_global_pointer_cache(
                        statements,
                        function,
                        ephemeral_locals,
                        return_branches,
                        label_positions,
                        pending_gotos,
                        entry_alias,
                    )?;
                }
                ArmBody::Return(value) => {
                    let result = match function.return_type {
                        Type::Float | Type::Double => Eabi::float_result().number,
                        _ => Eabi::general_result().number,
                    };
                    self.evaluate(value, function.return_type, result)?;
                    return_branches.push(self.output.instructions.len());
                    self.output.instructions.push(Instruction::Branch {
                        target: super::structured_early_return_schedule::
                            STRUCTURED_EPILOGUE_PLACEHOLDER,
                    });
                }
            }
        }
        let join = self.output.instructions.len();
        self.reset_structured_switch_edge_caches();

        for branch in join_branches {
            let Instruction::Branch { target } = &mut self.output.instructions[branch] else {
                unreachable!("a sparse switch join changed form")
            };
            *target = join;
        }
        for (index, target) in dispatch_patches {
            let destination = match target {
                crate::switch::Target::Body(body) => body_start[body],
                crate::switch::Target::Default => default_start,
            };
            match &mut self.output.instructions[index] {
                Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target } => *target = destination,
                _ => unreachable!("switch patch points at a non-branch instruction"),
            }
        }
        self.output.anonymous_label_bump +=
            super::structured_switch_lowering::canonical_switch_hidden_label_count(
                function, scrutinee, arms, default,
            );
        Ok(())
    }

    fn lower_shared_case_range(
        &mut self,
        register: u8,
        ranges: &[CaseRange],
        lo: usize,
        hi: usize,
        lower_bound: Option<i64>,
        upper_bound: Option<i64>,
        prefer_lower_root: bool,
        prefer_upper_pair: bool,
        patches: &mut Vec<(usize, crate::switch::Target)>,
    ) {
        if lo == hi {
            self.emit_shared_case_leaf(register, ranges[lo], lower_bound, upper_bound, patches);
            return;
        }
        if hi == lo + 1
            && lower_bound == Some(ranges[lo].low)
            && upper_bound.is_none()
            && ranges[lo].high.checked_add(1) == Some(ranges[hi].low)
            && ranges[hi].low != ranges[hi].high
        {
            self.push_sparse_compare(register, ranges[hi].high + 1);
            self.push_sparse_conditional(
                patches,
                (4, 0),
                crate::switch::Target::Default,
            );
            self.push_sparse_compare(register, ranges[hi].low);
            self.push_sparse_conditional(
                patches,
                (4, 0),
                crate::switch::Target::Body(ranges[hi].body),
            );
            self.push_sparse_branch(
                patches,
                crate::switch::Target::Body(ranges[lo].body),
            );
            return;
        }

        let mid = shared_case_pivot(
            ranges,
            lo,
            hi,
            upper_bound,
            prefer_lower_root,
            prefer_upper_pair,
        )
            .expect("a retained shared-body range has a supported pivot");
        let pivot = ranges[mid];
        debug_assert_eq!(pivot.low, pivot.high);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: register,
                immediate: pivot.low as i16,
            });
        self.push_sparse_conditional(patches, (12, 2), crate::switch::Target::Body(pivot.body));
        let right_branch = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 0,
                target: 0,
            });

        if mid == lo {
            self.push_sparse_branch(patches, crate::switch::Target::Default);
        } else {
            self.lower_shared_case_range(
                register,
                ranges,
                lo,
                mid - 1,
                lower_bound,
                Some(pivot.low - 1),
                false,
                prefer_upper_pair,
                patches,
            );
        }
        if mid == hi {
            patches.push((right_branch, crate::switch::Target::Default));
        } else if mid + 1 == hi
            && upper_bound == Some(ranges[hi].high)
            && ranges[hi].low == ranges[hi].high
            && pivot.high.checked_add(1) == Some(ranges[hi].low)
        {
            patches.push((
                right_branch,
                crate::switch::Target::Body(ranges[hi].body),
            ));
        } else {
            let right_entry = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } =
                &mut self.output.instructions[right_branch]
            {
                *target = right_entry;
            }
            self.lower_shared_case_range(
                register,
                ranges,
                mid + 1,
                hi,
                Some(pivot.high + 1),
                upper_bound,
                false,
                prefer_upper_pair,
                patches,
            );
        }
    }

    fn emit_shared_case_leaf(
        &mut self,
        register: u8,
        range: CaseRange,
        lower_bound: Option<i64>,
        upper_bound: Option<i64>,
        patches: &mut Vec<(usize, crate::switch::Target)>,
    ) {
        let body = crate::switch::Target::Body(range.body);
        if range.low == range.high {
            let value = range.low;
            if lower_bound == Some(value) && upper_bound == Some(value) {
                self.push_sparse_branch(patches, body);
            } else if upper_bound == Some(value) {
                self.push_sparse_compare(register, value);
                self.push_sparse_conditional(patches, (4, 0), body);
                self.push_sparse_branch(patches, crate::switch::Target::Default);
            } else if lower_bound == Some(value) {
                self.push_sparse_compare(register, value + 1);
                self.push_sparse_conditional(patches, (4, 0), crate::switch::Target::Default);
                self.push_sparse_branch(patches, body);
            } else {
                self.push_sparse_compare(register, value);
                self.push_sparse_conditional(patches, (12, 2), body);
                self.push_sparse_branch(patches, crate::switch::Target::Default);
            }
            return;
        }

        if upper_bound != Some(range.high) {
            self.push_sparse_compare(register, range.high + 1);
            self.push_sparse_conditional(patches, (4, 0), crate::switch::Target::Default);
        }
        if lower_bound == Some(range.low) {
            self.push_sparse_branch(patches, body);
        } else {
            self.push_sparse_compare(register, range.low);
            self.push_sparse_conditional(patches, (4, 0), body);
            self.push_sparse_branch(patches, crate::switch::Target::Default);
        }
    }

    fn push_sparse_compare(&mut self, register: u8, immediate: i64) {
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: register,
                immediate: immediate as i16,
            });
    }

    fn push_sparse_conditional(
        &mut self,
        patches: &mut Vec<(usize, crate::switch::Target)>,
        options: (u8, u8),
        target: crate::switch::Target,
    ) {
        let index = self.output.instructions.len();
        self.structured_switch_dispatch_conditionals.insert(index);
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: options.0,
                condition_bit: options.1,
                target: 0,
            });
        patches.push((index, target));
    }

    fn push_sparse_branch(
        &mut self,
        patches: &mut Vec<(usize, crate::switch::Target)>,
        target: crate::switch::Target,
    ) {
        let index = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });
        patches.push((index, target));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::SwitchArm;

    #[test]
    fn recognizes_sparse_labels_that_share_the_following_body() {
        let arms = vec![
            SwitchArm {
                value: 4,
                body: ArmBody::Statements(Vec::new()),
                falls_through: true,
            },
            SwitchArm {
                value: 5,
                body: ArmBody::Statements(vec![Statement::Return(None)]),
                falls_through: false,
            },
        ];

        assert!(arms.iter().any(|arm| arm.falls_through));
        assert!(is_sparse_retained_switch(&arms));
    }

    #[test]
    fn chooses_the_upper_pair_pivot_for_shared_fallthrough_bodies() {
        let arms = [0, 16, 1, 17]
            .into_iter()
            .enumerate()
            .map(|(index, value)| SwitchArm {
                value,
                body: ArmBody::Statements((index % 2 == 1)
                    .then(|| Statement::Return(None))
                    .into_iter()
                    .collect()),
                falls_through: index % 2 == 0,
            })
            .collect::<Vec<_>>();
        let ranges = shared_case_ranges(&arms);

        assert_eq!(shared_case_pivot(&ranges, 0, 3, None, false, true), Some(2));
        assert_eq!(shared_case_pivot(&ranges, 0, 1, Some(15), false, true), Some(1));
    }

    #[test]
    fn recognizes_a_small_nonfallthrough_comparison_tree() {
        let arms = [0, 1796, 1797, 1798]
            .into_iter()
            .map(|value| SwitchArm {
                value,
                body: ArmBody::Statements(vec![Statement::Return(None)]),
                falls_through: false,
            })
            .collect::<Vec<_>>();
        let ranges = shared_case_ranges(&arms);

        assert!(is_sparse_retained_switch(&arms));
        assert_eq!(shared_case_pivot(&ranges, 0, 3, None, true, false), Some(1));
        assert_eq!(shared_case_pivot(&ranges, 2, 3, None, false, false), Some(3));
    }

    #[test]
    fn chooses_the_upper_root_for_contiguous_nonfallthrough_cases() {
        let arms = (0..4)
            .map(|value| SwitchArm {
                value,
                body: ArmBody::Statements(vec![Statement::Return(None)]),
                falls_through: false,
            })
            .collect::<Vec<_>>();
        let ranges = shared_case_ranges(&arms);

        assert!(case_ranges_are_contiguous(&ranges));
        assert_eq!(shared_case_pivot(&ranges, 0, 3, None, false, false), Some(2));
    }

    #[test]
    fn leaves_dense_nonfallthrough_switches_to_the_jump_table_owner() {
        let arms = [3, 4, 5, 6, 7, 8, 9]
            .into_iter()
            .map(|value| SwitchArm {
                value,
                body: ArmBody::Statements(vec![Statement::Return(None)]),
                falls_through: false,
            })
            .collect::<Vec<_>>();

        assert!(!is_sparse_retained_switch(&arms));
    }

    #[test]
    fn materializes_the_saved_owner_for_a_sparse_switch_tail_call() {
        let mut instructions = vec![
            Instruction::Or { a: 3, s: 31, b: 31 },
            Instruction::AddImmediate {
                d: 4,
                a: 0,
                immediate: 128,
            },
            Instruction::Or { a: 5, s: 30, b: 30 },
            Instruction::BranchAndLink {
                target: "reply".to_owned(),
            },
            Instruction::Branch { target: 4 },
        ];

        assert!(rewrite_sparse_switch_tail_argument_copy(
            &mut instructions,
            31,
        ));
        assert!(matches!(
            instructions[0],
            Instruction::AddImmediate {
                d: 3,
                a: 31,
                immediate: 0,
            }
        ));
    }

    #[test]
    fn recognizes_seven_labels_when_shared_ranges_fit_the_tree() {
        let mut arms = [2, 3, 4]
            .into_iter()
            .map(|value| SwitchArm {
                value,
                body: ArmBody::Statements(vec![Statement::Return(None)]),
                falls_through: false,
            })
            .collect::<Vec<_>>();
        arms.extend([
            SwitchArm {
                value: 1,
                body: ArmBody::Statements(Vec::new()),
                falls_through: true,
            },
            SwitchArm {
                value: 6,
                body: ArmBody::Statements(Vec::new()),
                falls_through: true,
            },
            SwitchArm {
                value: 7,
                body: ArmBody::Statements(vec![Statement::Return(None)]),
                falls_through: false,
            },
            SwitchArm {
                value: 5,
                body: ArmBody::Statements(vec![Statement::Return(None)]),
                falls_through: false,
            },
        ]);

        assert!(arms.iter().any(|arm| arm.falls_through));
        assert!(is_sparse_retained_switch(&arms));
        let ranges = shared_case_ranges(&arms);
        assert_eq!(ranges.len(), 6);
        assert_eq!(shared_case_pivot(&ranges, 4, 5, None, false, true), Some(4));
    }

    #[test]
    fn coalesces_only_adjacent_labels_with_the_same_source_body() {
        let arms = vec![
            SwitchArm {
                value: 4,
                body: ArmBody::Statements(Vec::new()),
                falls_through: true,
            },
            SwitchArm {
                value: 5,
                body: ArmBody::Statements(Vec::new()),
                falls_through: true,
            },
            SwitchArm {
                value: 13,
                body: ArmBody::Statements(Vec::new()),
                falls_through: true,
            },
            SwitchArm {
                value: 15,
                body: ArmBody::Statements(vec![Statement::Return(None)]),
                falls_through: false,
            },
        ];

        let ranges = shared_case_ranges(&arms);
        assert_eq!(ranges.len(), 3);
        assert_eq!((ranges[0].low, ranges[0].high), (4, 5));
        assert_eq!((ranges[1].low, ranges[1].high), (13, 13));
        assert_eq!((ranges[2].low, ranges[2].high), (15, 15));
        assert!(ranges.iter().all(|range| range.body == 3));
        assert!(dispatch_range_is_supported(
            &ranges,
            0,
            ranges.len() - 1,
            None,
            false,
            true,
        ));
    }
}
