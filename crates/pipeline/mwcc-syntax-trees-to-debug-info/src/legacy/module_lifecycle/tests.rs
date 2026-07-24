use super::*;
use mwcc_machine_code::{Relocation, RelocationTarget};

#[test]
fn line_plan_maps_argument_setup_calls_and_epilogues() {
    let functions = [
        function(
            "_prolog",
            vec![
                call("construct", vec![variable("_ctors")]),
                call("start", vec![]),
            ],
        ),
        function(
            "_epilog",
            vec![
                call("stop", vec![]),
                call("destruct", vec![variable("_dtors")]),
            ],
        ),
        function("_unresolved", vec![call("missing", vec![])]),
    ];
    let sources = [
        source(3, &[4, 5], 6),
        source(8, &[9, 10], 11),
        source(13, &[14], 15),
    ];
    let machines = [
        machine(
            "_prolog",
            &[
                (3, RelocationKind::Addr16Ha, "_ctors"),
                (4, RelocationKind::Addr16Lo, "_ctors"),
                (5, RelocationKind::Rel24, "construct"),
                (6, RelocationKind::Rel24, "start"),
            ],
        ),
        machine(
            "_epilog",
            &[
                (3, RelocationKind::Rel24, "stop"),
                (4, RelocationKind::Addr16Ha, "_dtors"),
                (5, RelocationKind::Addr16Lo, "_dtors"),
                (6, RelocationKind::Rel24, "destruct"),
            ],
        ),
        machine(
            "_unresolved",
            &[(3, RelocationKind::Rel24, "missing")],
        ),
    ];
    let layout = FunctionLayout {
        order: vec![0, 1, 2],
        offsets: vec![0, 44, 88],
        sizes: vec![44, 44, 32],
        byte_len: 120,
    };
    let paired = functions
        .iter()
        .zip(sources)
        .map(|(function, source)| (function, source))
        .collect::<Vec<_>>();

    let rows = line_records(&paired, &machines, &layout).unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| (row.line, row.address_delta))
            .collect::<Vec<_>>(),
        vec![
            (3, 0),
            (4, 12),
            (5, 24),
            (6, 28),
            (8, 44),
            (9, 56),
            (10, 60),
            (11, 72),
            (13, 88),
            (14, 100),
            (15, 104),
        ]
    );
}

#[test]
fn unsized_callable_array_relocates_its_shared_element_type() {
    let DebugRecord::Entry(entry) =
        callable_array(DebugEntryId(2), DebugEntryId(3), DebugEntryId(1))
    else {
        unreachable!()
    };
    assert_eq!(entry.tag, Tag::ArrayType);
    let AttributeValue::RelocatableBlock2(block) = &entry.attributes[1].value else {
        panic!("array has no relocatable subscript descriptor");
    };
    assert_eq!(
        &block.bytes[..11],
        &[0, 0, 10, 0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]
    );
    assert_eq!(block.relocations[0].offset, 18);
    assert_eq!(
        block.relocations[0].address,
        Address::debug_entry(DebugEntryId(1))
    );
}

fn function(name: &str, statements: Vec<Statement>) -> Function {
    Function {
        return_type: Type::Void,
        name: name.into(),
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

fn call(name: &str, arguments: Vec<Expression>) -> Statement {
    Statement::Expression(Expression::Call {
        name: name.into(),
        arguments,
    })
}

fn variable(name: &str) -> Expression {
    Expression::Variable(name.into())
}

fn source(start: u32, statements: &[u32], end: u32) -> FunctionSource {
    FunctionSource {
        body_start_line: start,
        local_lines: Vec::new(),
        statement_lines: statements.to_vec(),
        leaf_statement_lines: statements.to_vec(),
        control_flow_lines: Vec::new(),
        terminal_return_line: None,
        body_end_line: end,
    }
}

fn machine(name: &str, relocations: &[(usize, RelocationKind, &str)]) -> MachineFunction {
    MachineFunction {
        name: name.into(),
        relocations: relocations
            .iter()
            .map(|(instruction_index, kind, target)| Relocation {
                instruction_index: *instruction_index,
                kind: *kind,
                target: RelocationTarget::External((*target).into()),
            })
            .collect(),
        ..MachineFunction::default()
    }
}
