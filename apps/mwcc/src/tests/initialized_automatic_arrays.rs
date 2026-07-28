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

#[test]
fn pools_multiple_initialized_arrays_into_a_dense_copy_transaction() {
    let source = br#"
        void consume(char*);

        void initialize(void) {
            char date[32] = "";
            char time[32] = "";
            char ampm[32] = "";
            char buffer[256] = "";
            consume(date);
            consume(time);
            consume(ampm);
            consume(buffer);
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "pooled-initialized-automatic-arrays.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("multiple initialized arrays should use the pooled structured frame");

    // stmw r14,...(r1) owns the fixed direct-copy window and the tail loop's
    // count register.
    assert!(object.windows(2).any(|bytes| bytes == [0xbd, 0xc1]));

    // mtctr r14; lwz r5,4(r3); lwzu r0,8(r3); stw r5,4(r4);
    // stwu r0,8(r4); bdnz
    let tail_copy = [
        0x7d, 0xc9, 0x03, 0xa6, 0x80, 0xa3, 0x00, 0x04, 0x84, 0x03, 0x00, 0x08, 0x90, 0xa4, 0x00,
        0x04, 0x94, 0x04, 0x00, 0x08, 0x42, 0x00, 0xff, 0xf0,
    ];
    assert!(object
        .windows(tail_copy.len())
        .any(|bytes| bytes == tail_copy));
}
