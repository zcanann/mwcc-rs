use crate::{compile, SourceLanguage};

#[test]
fn preserves_a_float_parameter_across_a_call_in_a_compact_fpr_lane() {
    let source = br#"
        typedef struct Shape {
            int padding[7];
            float radius;
        } Shape;
        extern void set_endpoints(Shape*, void*, void*);

        void set(Shape* shape, void* start, void* end, float radius) {
            set_endpoints(shape, start, end);
            shape->radius = radius;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "saved-float-parameter.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_7,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("mixed GPR/FPR call survivors should compile");

    // Exact GC/2.7 output measured with mwcceppc. GameCube saves only the
    // scalar double lane for f31; the paired-single half is a Wii ABI concern.
    let expected = [
        0x94, 0x21, 0xff, 0xe0, // stwu r1,-32(r1)
        0x7c, 0x08, 0x02, 0xa6, // mflr r0
        0x90, 0x01, 0x00, 0x24, // stw r0,36(r1)
        0xdb, 0xe1, 0x00, 0x18, // stfd f31,24(r1)
        0x93, 0xe1, 0x00, 0x14, // stw r31,20(r1)
        0x7c, 0x7f, 0x1b, 0x78, // mr r31,r3
        0xff, 0xe0, 0x08, 0x90, // fmr f31,f1
        0x48, 0x00, 0x00, 0x01, // bl set_endpoints
        0xd3, 0xff, 0x00, 0x1c, // stfs f31,28(r31)
        0xcb, 0xe1, 0x00, 0x18, // lfd f31,24(r1)
        0x83, 0xe1, 0x00, 0x14, // lwz r31,20(r1)
        0x80, 0x01, 0x00, 0x24, // lwz r0,36(r1)
        0x7c, 0x08, 0x03, 0xa6, // mtlr r0
        0x38, 0x21, 0x00, 0x20, // addi r1,r1,32
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}
