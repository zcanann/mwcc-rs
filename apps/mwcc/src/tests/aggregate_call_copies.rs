use crate::{compile, SourceLanguage};

#[test]
fn copies_initialized_one_word_aggregates_for_a_terminal_call() {
    let source = br#"
        typedef struct Color {
            unsigned char red;
            unsigned char green;
            unsigned char blue;
            unsigned char alpha;
        } Color;

        extern unsigned short font_encoding(void);
        extern unsigned char language(void);
        extern const char *japanese;
        extern const char *english;
        extern const char *europe[];
        void compiled(void) {
            const char *message;
            Color background = { 0, 0, 0, 0 };
            Color foreground = { 0xff, 0xff, 0xff, 0 };
            if (television_format() == 0) {
                if (font_encoding() == 1) {
                    message = japanese;
                } else {
                    message = english;
                }
            } else {
                message = europe[language()];
            }
            fatal(foreground, background, message);
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let config = mwcc_versions::CompilerConfig {
        build: mwcc_versions::GC_1_2_5N,
        flags,
    };
    let object = compile(
        source,
        "aggregate-call-copies.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the aggregate-copy call should compile");

    let expected = [
        0x7c, 0x08, 0x02, 0xa6, // mflr r0
        0x90, 0x01, 0x00, 0x04, // stw r0,4(r1)
        0x94, 0x21, 0xff, 0xe8, // stwu r1,-24(r1)
        0x80, 0x60, 0x00, 0x00, // lwz r3,@background(0)
        0x80, 0x00, 0x00, 0x00, // lwz r0,@foreground(0)
        0x90, 0x61, 0x00, 0x14, // stw r3,20(r1)
        0x90, 0x01, 0x00, 0x10, // stw r0,16(r1)
        0x48, 0x00, 0x00, 0x01, // bl television_format
        0x2c, 0x03, 0x00, 0x00, // cmpwi r3,0
        0x40, 0x82, 0x00, 0x24, // bne non-NTSC
        0x48, 0x00, 0x00, 0x01, // bl font_encoding
        0x54, 0x60, 0x04, 0x3e, // clrlwi r0,r3,16
        0x28, 0x00, 0x00, 0x01, // cmplwi r0,1
        0x40, 0x82, 0x00, 0x0c, // bne english
        0x80, 0xa0, 0x00, 0x00, // lwz r5,japanese@sda21
        0x48, 0x00, 0x00, 0x24, // b join
        0x80, 0xa0, 0x00, 0x00, // lwz r5,english@sda21
        0x48, 0x00, 0x00, 0x1c, // b join
        0x48, 0x00, 0x00, 0x01, // bl language
        0x3c, 0x80, 0x00, 0x00, // lis r4,europe@ha
        0x54, 0x63, 0x15, 0xba, // rlwinm r3,r3,2,22,29
        0x38, 0x04, 0x00, 0x00, // addi r0,r4,europe@l
        0x7c, 0x60, 0x1a, 0x14, // add r3,r0,r3
        0x80, 0xa3, 0x00, 0x00, // lwz r5,0(r3)
        0x80, 0xc1, 0x00, 0x14, // lwz r6,20(r1)
        0x38, 0x81, 0x00, 0x08, // addi r4,r1,8
        0x80, 0x01, 0x00, 0x10, // lwz r0,16(r1)
        0x38, 0x61, 0x00, 0x0c, // addi r3,r1,12
        0x90, 0xc1, 0x00, 0x08, // stw r6,8(r1)
        0x90, 0x01, 0x00, 0x0c, // stw r0,12(r1)
        0x48, 0x00, 0x00, 0x01, // bl fatal
        0x80, 0x01, 0x00, 0x1c, // lwz r0,28(r1)
        0x38, 0x21, 0x00, 0x18, // addi r1,r1,24
        0x7c, 0x08, 0x03, 0xa6, // mtlr r0
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    let matched = object
        .windows(expected.len())
        .any(|bytes| bytes == expected);
    if !matched {
        let start = object
            .windows(4)
            .position(|bytes| bytes == [0x7c, 0x08, 0x02, 0xa6])
            .expect("compiled text has an mflr prologue");
        panic!(
            "aggregate-copy text differs: {:02x?}",
            &object[start..start + expected.len()]
        );
    }

    let expected_externals = [
        "television_format",
        "font_encoding",
        "language",
        "fatal",
    ];
    let external_order = super::elf_object::symbols(&object)
        .into_iter()
        .map(|(name, _, _, _)| name)
        .filter(|name| expected_externals.contains(&name.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(external_order, expected_externals);
}

#[test]
fn function_first_symbols_keep_body_creation_order_after_the_function() {
    let source = br#"
        extern int explicit_a(void);
        extern int explicit_b(void);

        void compiled(void) {
            implicit_first();
            explicit_a();
            explicit_b();
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.emit_mwcats = false;
    let config = mwcc_versions::CompilerConfig {
        build: mwcc_versions::GC_1_2_5N,
        flags,
    };
    let object = compile(
        source,
        "function-first-symbols.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the mixed implicit/explicit call sequence should compile");

    let expected = ["compiled", "implicit_first", "explicit_a", "explicit_b"];
    let order = super::elf_object::symbols(&object)
        .into_iter()
        .map(|(name, _, _, _)| name)
        .filter(|name| expected.contains(&name.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(order, expected);
}
