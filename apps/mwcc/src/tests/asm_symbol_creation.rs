use crate::{compile, SourceLanguage};

#[test]
fn asm_references_register_symbols_in_declaration_and_operand_order() {
    let source = br#"
        static void static_target(void);
        extern void branch_target(void);
        extern int absolute_target;

        asm void entry(void) {
            nofralloc
            bl branch_target
            lis r3, absolute_target@ha
            addi r3, r3, absolute_target@l
            bl static_target
            blr
        }

        static void helper(void) {}
        static void static_target(void) {}
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.emit_mwcats = false;
    let config = mwcc_versions::CompilerConfig {
        build: mwcc_versions::GC_1_2_5,
        flags,
    };
    let object = compile(
        source,
        "asm-symbol-creation.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the asm symbol-order probe should compile");

    let expected = [
        "static_target",
        "helper",
        "entry",
        "branch_target",
        "absolute_target",
    ];
    let order = super::elf_object::symbols(&object)
        .into_iter()
        .map(|(name, _, _, _)| name)
        .filter(|name| expected.contains(&name.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(order, expected);
}

#[test]
fn gc_1_1p1_asm_registers_absolute_data_before_each_function() {
    let source = br#"
        extern int state;
        extern int flags;

        asm void save(void) {
            nofralloc
            lis r3, state@h
            ori r3, r3, state@l
            blr
        }

        asm void restore(void) {
            nofralloc
            lis r3, state@h
            ori r3, r3, state@l
            lis r4, flags@h
            ori r4, r4, flags@l
            blr
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.emit_mwcats = false;
    flags.inline_deferred = true;
    let object = compile(
        source,
        "gc-1.1p1-asm-symbols.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_1P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the GC/1.1p1 asm symbol-order probe should compile");

    let expected = ["state", "save", "flags", "restore"];
    let order = super::elf_object::symbols(&object)
        .into_iter()
        .map(|(name, _, _, _)| name)
        .filter(|name| expected.contains(&name.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(order, expected);
}
