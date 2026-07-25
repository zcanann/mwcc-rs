use super::*;

#[test]
fn pointer_array_subscript_keeps_its_aggregate_reference() {
    let DebugRecord::Entry(entry) =
        array_type(DebugEntryId(4), DebugEntryId(5), 503, DebugEntryId(3))
    else {
        unreachable!()
    };
    assert_eq!(entry.tag, Tag::ArrayType);
    let AttributeValue::RelocatableBlock2(block) = &entry.attributes[1].value else {
        panic!("pointer array has no relocatable subscript descriptor");
    };
    assert_eq!(
        block.bytes,
        [0, 0, 10, 0, 0, 0, 0, 0, 0, 1, 0xf6, 8, 0, 0x83, 0, 5, 1, 0, 0, 0, 0,]
    );
    assert_eq!(block.relocations[0].offset, 17);
    assert_eq!(
        block.relocations[0].address,
        Address::debug_entry(DebugEntryId(3))
    );
}

#[test]
fn lifecycle_lines_cover_each_store_and_epilogue() {
    let functions = [function("install"), function("clear")];
    let sources = [source(11, 12), source(15, 16)];
    let machines = [
        MachineFunction {
            name: "install".into(),
            ..MachineFunction::default()
        },
        MachineFunction {
            name: "clear".into(),
            ..MachineFunction::default()
        },
    ];
    let layout = FunctionLayout {
        order: vec![0, 1],
        offsets: vec![0, 20],
        sizes: vec![20, 16],
        byte_len: 36,
    };
    let paired = functions
        .iter()
        .zip(sources)
        .map(|(function, source)| (function, source))
        .collect::<Vec<_>>();

    let records = line_records(&paired, &machines, &layout).unwrap();
    assert_eq!(
        records
            .iter()
            .map(|record| (record.line, record.address_delta))
            .collect::<Vec<_>>(),
        [(11, 0), (12, 16), (15, 20), (16, 32)]
    );
}

fn function(name: &str) -> Function {
    Function {
        return_type: Type::Void,
        name: name.into(),
        is_static: false,
        is_weak: false,
        parameters: Vec::new(),
        locals: Vec::new(),
        statements: vec![Statement::Store {
            target: Expression::Variable("active".into()),
            value: Expression::IntegerLiteral(0),
        }],
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

fn source(statement: u32, end: u32) -> FunctionSource {
    FunctionSource {
        body_start_line: statement - 1,
        local_lines: Vec::new(),
        statement_lines: vec![statement],
        leaf_statement_lines: vec![statement],
        control_flow_lines: Vec::new(),
        terminal_return_line: None,
        body_end_line: end,
    }
}
