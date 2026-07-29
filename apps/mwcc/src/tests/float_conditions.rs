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

#[test]
fn selects_a_computed_float_with_a_logical_range_condition() {
    let source = br#"
        void compiled(
            float value,
            float lower,
            float upper,
            float numerator,
            float* output
        ) {
            *output = value >= lower && value <= upper
                ? 0.0f
                : numerator / value;
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
        "logical-range-float-select.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a logical range condition should select between computed float arms");

    // The false arm remains path-local and performs the source division only
    // after the logical condition branches to it.
    let divide = [0xec, 0x04, 0x08, 0x24]; // fdivs f0,f4,f1
    assert!(object.windows(divide.len()).any(|bytes| bytes == divide));
}

#[test]
fn preserves_a_member_base_while_loading_a_global_float_operand() {
    let source = br#"
        struct Limits {
            char padding[356];
            float bound;
        };
        struct Fighter {
            char padding[240];
            float velocity;
        };
        extern struct Limits* limits;
        void compiled(struct Fighter* fighter) {
            if (fighter->velocity > limits->bound) {
                fighter->velocity = 0.0f;
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
        "member-global-float-condition.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a member/global float comparison should compile");

    // The left member keeps `fighter` live in r3 while the right operand uses
    // r4 for the absolute global pointer base.
    let comparison = [
        0xc0, 0x23, 0x00, 0xf0, // lfs f1,240(r3)
        0x80, 0x80, 0x00, 0x00, // lwz r4,limits
        0xc0, 0x04, 0x01, 0x64, // lfs f0,356(r4)
        0xfc, 0x01, 0x00, 0x40, // fcmpo cr0,f1,f0
    ];
    assert!(
        object
            .windows(comparison.len())
            .any(|bytes| bytes == comparison),
        "the global load must not overwrite the live fighter base"
    );
}
