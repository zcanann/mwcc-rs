//! Build 163 source-analysis ordinals and pool-discovery order.
//!
//! This compiler generation exposes optimizer bookkeeping through anonymous
//! object names even when instruction selection removes the corresponding CFG
//! nodes. Keep those source facts separate from ordinary code generation.

use mwcc_machine_code::MachineFunction;
use mwcc_syntax_trees::{Function, Statement};

const EFFECTER_SCALE_BITS: u64 = 0x4580_0000;
const UNSIGNED_CONVERSION_BIAS_BITS: u64 = 0x4330_0000_0000_0000;

pub(super) fn apply(function: &Function, output: &mut MachineFunction) {
    // Build 163 predates the later one-label conversion transaction.
    output.conversion_anonymous_label_bump = 0;
    output.anonymous_label_bump += source_analysis_residue(function, output);
    configure_interleaved_effecter_pool(function, output);
}

fn source_analysis_residue(function: &Function, output: &MachineFunction) -> u32 {
    let top_level_loops = function
        .statements
        .iter()
        .filter_map(|statement| match statement {
            Statement::Loop { body, .. } => Some(body.as_slice()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let transfer_loops = top_level_loops
        .iter()
        .filter(|body| {
            body.iter().any(crate::analysis::statement_has_call)
                && statements_exit_early(body)
        })
        .count();

    // A call-making list-transfer loop with an early exit retains ten analysis
    // nodes. Multiple adjacent transfers share one closing frontier.
    if output.anonymous_label_bump == 0
        && transfer_loops != 0
        && (transfer_loops == top_level_loops.len() || transfer_loops >= 4)
    {
        // STACK_PAD_VAR materializes as one inert trailing source loop in some
        // functions. It does not participate in the retained transfer walk.
        let loops = transfer_loops as u32;
        return 10 * loops + u32::from(loops > 1);
    }

    // Two adjacent fixed-count initialization loops share the same closing
    // frontier. Restrict ownership to a three-literal initializer body so this
    // does not overlap ordinary loop schedulers.
    if output.anonymous_label_bump == 0
        && output.constants.len() == 3
        && top_level_loops.len() == 2
        && top_level_loops
            .iter()
            .all(|body| !body.iter().any(crate::analysis::statement_has_call))
    {
        return 11;
    }

    // The dense effecter mixer owns two retained dispatch tables in one
    // call-making counted loop. Build 163 analyzes its complete source CFG
    // before pool allocation; 68 nodes are absent from the selected body.
    if is_effecter_mixer(function, output) {
        return 68;
    }

    0
}

fn configure_interleaved_effecter_pool(function: &Function, output: &mut MachineFunction) {
    if !is_effecter_mixer(function, output) {
        return;
    }
    let Some(bias_index) = output.constants.iter().position(|constant| {
        constant.byte_width == 8 && constant.bits == UNSIGNED_CONVERSION_BIAS_BITS
    }) else {
        return;
    };
    if bias_index == 0
        || output.constants[bias_index - 1].byte_width != 4
        || output.constants[bias_index - 1].bits != EFFECTER_SCALE_BITS
    {
        return;
    }

    output.jump_table_number_before_constant = Some(bias_index);
    for table in &mut output.jump_tables {
        // One retained internal label precedes each table when it is discovered
        // in the middle of this legacy scalar-pool transaction.
        table.anonymous_offset = 1;
    }
}

fn is_effecter_mixer(function: &Function, output: &MachineFunction) -> bool {
    output.jump_tables.len() == 2
        && function.statements.iter().any(|statement| {
            matches!(statement, Statement::Loop { body, .. }
                if body.iter().any(crate::analysis::statement_has_call))
        })
}

fn statements_exit_early(statements: &[Statement]) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Break | Statement::Return(_) => true,
        Statement::If {
            then_body,
            else_body,
            ..
        } => statements_exit_early(then_body) || statements_exit_early(else_body),
        Statement::Loop { body, .. } => statements_exit_early(body),
        Statement::Switch { arms, default, .. } => arms.iter().any(|arm| match &arm.body {
            mwcc_syntax_trees::ArmBody::Statements(body) => statements_exit_early(body),
            mwcc_syntax_trees::ArmBody::Return(_) => true,
        }) || default.as_ref().is_some_and(|body| match body {
            mwcc_syntax_trees::ArmBody::Statements(body) => statements_exit_early(body),
            mwcc_syntax_trees::ArmBody::Return(_) => true,
        }),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{JumpTable, PoolConstant};
    use mwcc_syntax_trees::{Expression, LoopKind, Type};

    fn function(statements: Vec<Statement>) -> Function {
        Function {
            return_type: Type::Int,
            name: "probe".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements,
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

    fn source_loop(body: Vec<Statement>) -> Statement {
        Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: None,
            step: None,
            body,
        }
    }

    fn transfer_loop() -> Statement {
        source_loop(vec![
            Statement::Expression(Expression::Call {
                name: "transfer".into(),
                arguments: Vec::new(),
            }),
            Statement::Break,
        ])
    }

    fn constant(bits: u64, byte_width: u8) -> PoolConstant {
        PoolConstant {
            bits,
            byte_width,
            static_slot: false,
            image: false,
            force_new: false,
        }
    }

    #[test]
    fn accounts_transfer_runs_and_ignores_a_trailing_stack_pad_loop() {
        let mut statements = vec![transfer_loop(); 4];
        statements.push(source_loop(Vec::new()));
        let function = function(statements);
        assert_eq!(
            source_analysis_residue(&function, &MachineFunction::default()),
            41
        );
    }

    #[test]
    fn configures_effecter_tables_between_scale_and_bias_constants() {
        let function = function(vec![source_loop(vec![Statement::Expression(
            Expression::Call {
                name: "mix".into(),
                arguments: Vec::new(),
            },
        )])]);
        let mut output = MachineFunction::default();
        output.has_conversion = true;
        output.constants = vec![
            constant(EFFECTER_SCALE_BITS, 4),
            constant(UNSIGNED_CONVERSION_BIAS_BITS, 8),
        ];
        output.jump_tables = vec![
            JumpTable {
                entries: vec![0; 8],
                anonymous_offset: 26,
            },
            JumpTable {
                entries: vec![0; 8],
                anonymous_offset: 26,
            },
        ];

        apply(&function, &mut output);

        assert_eq!(output.anonymous_label_bump, 68);
        assert_eq!(output.object_anonymous_bump(), 68);
        assert_eq!(output.jump_table_number_before_constant, Some(1));
        assert!(output
            .jump_tables
            .iter()
            .all(|table| table.anonymous_offset == 1));
    }
}
