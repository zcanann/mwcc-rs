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

#[test]
fn reuses_a_nested_source_base_across_an_inlined_vec3_setter() {
    let source = br#"
        namespace JGeometry {
            template <typename T> struct TVec3 { T x, y, z; };
            template <> struct TVec3<float> {
                float x, y, z;
                template <typename U> void set(const TVec3<U>& source) {
                    x = source.x;
                    y = source.y;
                    z = source.z;
                }
            };
            typedef TVec3<float> TVec3f;
        }
        struct Data { JGeometry::TVec3f source; };
        struct Block {
            Data* data;
            JGeometry::TVec3f output;
            void copy();
        };
        void Block::copy() { output.set(data->source); }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.rtti = false;
    flags.optimization = mwcc_versions::Optimization::O4;
    flags.optimization_goal = mwcc_versions::OptimizationGoal::Size;
    flags.inline_enabled = true;
    let object = compile(
        source,
        "nested-source-vec3-setter.cpp",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_6,
            flags,
        },
        Some(SourceLanguage::Cxx),
        None,
        false,
    )
    .expect("the nested-source Vec3 setter should compile");

    let expected = [
        0x80, 0x83, 0x00, 0x00, // lwz r4,0(r3)
        0xc0, 0x04, 0x00, 0x00, // lfs f0,0(r4)
        0xd0, 0x03, 0x00, 0x04, // stfs f0,4(r3)
        0xc0, 0x04, 0x00, 0x04, // lfs f0,4(r4)
        0xd0, 0x03, 0x00, 0x08, // stfs f0,8(r3)
        0xc0, 0x04, 0x00, 0x08, // lfs f0,8(r4)
        0xd0, 0x03, 0x00, 0x0c, // stfs f0,12(r3)
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}
