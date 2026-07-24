use crate::{compile, SourceLanguage};

#[test]
fn tests_a_float_member_against_zero_as_a_float() {
    let source = br#"
        struct State { float value; float result; };
        void compiled(struct State* state) {
            if (!state->value) {
                state->result = 0;
            }
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
        "float-member-condition.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a float member condition should compile");

    // lfs f1,0(r3); lfs f0,@zero@sda21(0); fcmpu cr0,f1,f0
    let expected = [
        0xc0, 0x23, 0x00, 0x00, 0xc0, 0x00, 0x00, 0x00, 0xfc, 0x01, 0x00, 0x00,
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}

#[test]
fn multiplies_a_constant_indexed_float_member_in_place() {
    let source = br#"
        typedef struct Vec3 { float x; float y; float z; } Vec3;
        float compiled(float scale, Vec3* values) {
            return scale * values[2].x;
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
        "indexed-float-member.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a constant-indexed float member should compile");

    // Exact GC/1.2.5n hard-float output measured with mwcceppc:
    // lfs f0,24(r3); fmuls f1,f1,f0; blr.
    let expected = [
        0xc0, 0x03, 0x00, 0x18, 0xec, 0x21, 0x00, 0x32, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}

#[test]
fn preserves_a_compound_float_member_assignment_value() {
    let source = br#"
        struct Frame { float current; float delta; float limit; };
        void compiled(struct Frame* frame) {
            if ((frame->current += frame->delta) >= frame->limit) {
                frame->limit = frame->current;
            }
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let config = mwcc_versions::CompilerConfig {
        build: mwcc_versions::GC_3_0A3,
        flags,
    };
    let object = compile(
        source,
        "compound-float-member.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a compound float member assignment should remain usable as a value");

    let update = [
        0xc0, 0x23, 0x00, 0x00, // lfs f1,0(r3)
        0xc0, 0x03, 0x00, 0x04, // lfs f0,4(r3)
        0xec, 0x21, 0x00, 0x2a, // fadds f1,f1,f0
        0xd0, 0x23, 0x00, 0x00, // stfs f1,0(r3)
    ];
    assert!(object.windows(update.len()).any(|bytes| bytes == update));
}
