use crate::{compile, SourceLanguage};

#[test]
fn folds_a_terminal_expression_assertion_after_label_resolution() {
    let source = br#"
        extern unsigned get_device(void);
        extern void report(unsigned, char*, int, char*, char*);
        extern void panic(char*, int, char*);

        struct Object { void* field; };

        void compiled(struct Object* object) {
            (object->field != 0
                ? (void) 0
                : (report(get_device(), "source.cpp", 485, "%s", "field != 0"),
                   panic("source.cpp", 485, "Halt")));
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "resolved-cold-epilogue.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_6,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the terminal assertion expression should compile");

    // GC/2.6 oracle .text. The bne at +0x14 reaches the linkage epilogue
    // directly; the expression-level labels must not leave an intervening b.
    let expected_text = [
        0x94, 0x21, 0xff, 0xf0, 0x7c, 0x08, 0x02, 0xa6, 0x90, 0x01, 0x00, 0x14, 0x80, 0x03, 0x00,
        0x00, 0x28, 0x00, 0x00, 0x00, 0x40, 0x82, 0x00, 0x38, 0x48, 0x00, 0x00, 0x01, 0x3c, 0x80,
        0x00, 0x00, 0x38, 0x84, 0x00, 0x00, 0x38, 0xa0, 0x01, 0xe5, 0x38, 0xc0, 0x00, 0x00, 0x3c,
        0xe0, 0x00, 0x00, 0x38, 0xe7, 0x00, 0x00, 0x48, 0x00, 0x00, 0x01, 0x3c, 0x60, 0x00, 0x00,
        0x38, 0x63, 0x00, 0x00, 0x38, 0x80, 0x01, 0xe5, 0x38, 0xa0, 0x00, 0x00, 0x48, 0x00, 0x00,
        0x01, 0x80, 0x01, 0x00, 0x14, 0x7c, 0x08, 0x03, 0xa6, 0x38, 0x21, 0x00, 0x10, 0x4e, 0x80,
        0x00, 0x20,
    ];
    assert!(object
        .windows(expected_text.len())
        .any(|bytes| bytes == expected_text));
}
