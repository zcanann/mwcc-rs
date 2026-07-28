use crate::{compile, SourceLanguage};

#[test]
fn zero_fills_the_implicit_tail_of_an_initialized_automatic_array() {
    let source = br#"
        void consume(char*);

        void initialize(void) {
            char buffer[32] = "";
            consume(buffer);
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "initialized-automatic-array.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("an initialized automatic array should compile through its structured frame");

    // li r0,0; li r3,8; addi r4,r1,slot-4; mtctr r3;
    // loop: stwu r0,4(r4); bdnz loop
    let zero_fill = [
        0x38, 0x00, 0x00, 0x00, 0x38, 0x60, 0x00, 0x08, 0x38, 0x81, 0x00, 0x04, 0x7c, 0x69, 0x03,
        0xa6, 0x94, 0x04, 0x00, 0x04, 0x42, 0x00, 0xff, 0xfc,
    ];
    assert!(object
        .windows(zero_fill.len())
        .any(|bytes| bytes == zero_fill));
}
