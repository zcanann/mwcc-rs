use crate::{compile, SourceLanguage};

#[test]
fn copies_a_vec3_member_into_another_member_with_mwccs_word_schedule() {
    let source = br#"
        struct Vec3 { float x, y, z; };
        struct Source { char pad[56]; struct Vec3 translation; };
        struct Target { char pad[6780]; struct Vec3 translation; };
        extern void touch(void);
        void compiled(struct Source* source, struct Target* target) {
            touch();
            *(&target->translation) = source->translation;
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
        "aggregate-member-copy.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a member-backed Vec3 copy should compile");

    let expected = [
        0x80, 0x7e, 0x00, 0x38, // lwz r3,56(r30)
        0x80, 0x1e, 0x00, 0x3c, // lwz r0,60(r30)
        0x90, 0x7f, 0x1a, 0x7c, // stw r3,6780(r31)
        0x90, 0x1f, 0x1a, 0x80, // stw r0,6784(r31)
        0x80, 0x1e, 0x00, 0x40, // lwz r0,64(r30)
        0x90, 0x1f, 0x1a, 0x84, // stw r0,6788(r31)
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}

#[test]
fn recombines_an_inlined_scalarized_vec3_copy() {
    let source = br#"
        struct Vec3 { float x, y, z; };
        struct Source { char pad[56]; struct Vec3 translation; };
        struct Target { char pad[6780]; struct Vec3 translation; };
        extern void touch(void);
        inline void copy(struct Source* source, struct Vec3* target) {
            *target = source->translation;
        }
        void compiled(struct Source* source, struct Target* target) {
            touch();
            copy(source, &target->translation);
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
        "scalarized-aggregate-member-copy.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("an inlined scalarized Vec3 copy should compile");

    let expected = [
        0x80, 0x7e, 0x00, 0x38, 0x80, 0x1e, 0x00, 0x3c, 0x90, 0x7f, 0x1a, 0x7c,
        0x90, 0x1f, 0x1a, 0x80, 0x80, 0x1e, 0x00, 0x40, 0x90, 0x1f, 0x1a, 0x84,
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}
