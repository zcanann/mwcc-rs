use crate::{compile, SourceLanguage};

#[test]
fn converts_a_computed_float_for_an_integer_member_store() {
    let source = br#"
        struct Value {
            float x;
            short y;
        };

        void convert(struct Value* value) {
            value->y = value->x * 4;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "float-to-integer-store.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_6,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the computed float store should compile");

    // Exact GC/2.6 output measured from mwcceppc.
    let expected = [
        0x94, 0x21, 0xff, 0xf0, // stwu r1,-16(r1)
        0xc0, 0x20, 0x00, 0x00, // lfs f1,@4@sda21(0)
        0xc0, 0x03, 0x00, 0x00, // lfs f0,0(r3)
        0xec, 0x01, 0x00, 0x32, // fmuls f0,f1,f0
        0xfc, 0x00, 0x00, 0x1e, // fctiwz f0,f0
        0xd8, 0x01, 0x00, 0x08, // stfd f0,8(r1)
        0x80, 0x01, 0x00, 0x0c, // lwz r0,12(r1)
        0xb0, 0x03, 0x00, 0x04, // sth r0,4(r3)
        0x38, 0x21, 0x00, 0x10, // addi r1,r1,16
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}

#[test]
fn plans_a_conversion_image_for_a_framed_explicit_cast() {
    let source = br#"
        extern void consume(int value);

        void convert(float* value) {
            consume((int)(*value * 4));
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    compile(
        source,
        "framed-float-to-integer-cast.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_6,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a non-leaf explicit conversion should have planned stack scratch");
}
