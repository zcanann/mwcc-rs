use crate::{compile, SourceLanguage};

#[test]
fn selects_between_float_members_into_a_member_store() {
    let source = br#"
        struct Frame {
            float maximum;
            float minimum;
            float current;
            int kind;
        };

        void init_frame(struct Frame* frame) {
            frame->current = frame->kind == 1 ? frame->maximum : frame->minimum;
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
        "member-float-select.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("two float members should form a load-select-store diamond");

    let expected = [
        0x80, 0x03, 0x00, 0x0c, // lwz r0,12(r3)
        0x2c, 0x00, 0x00, 0x01, // cmpwi r0,1
        0x40, 0x82, 0x00, 0x0c, // bne false
        0xc0, 0x03, 0x00, 0x00, // lfs f0,0(r3)
        0x48, 0x00, 0x00, 0x08, // b join
        0xc0, 0x03, 0x00, 0x04, // false: lfs f0,4(r3)
        0xd0, 0x03, 0x00, 0x08, // join: stfs f0,8(r3)
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}

#[test]
fn schedules_an_ipa_forwarded_float_member_initializer() {
    let source = br#"
        struct Frame {
            void* vtable;
            float maximum;
            float minimum;
            float current;
            float delta;
            int state;
            int kind;
            unsigned char alternate;
        };

        static void init_frame(struct Frame* frame);

        void initialize(
            struct Frame* frame,
            int kind,
            float maximum,
            float minimum,
            float delta
        ) {
            frame->kind = kind;
            frame->maximum = maximum;
            frame->minimum = minimum;
            frame->delta = delta;
            frame->state = 0;
            frame->alternate = 0;
            init_frame(frame);
        }

        static void init_frame(struct Frame* frame) {
            frame->current =
                frame->kind == 1 ? frame->maximum : frame->minimum;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.ipa_file = true;
    let config = mwcc_versions::CompilerConfig {
        build: mwcc_versions::GC_3_0A3,
        flags,
    };
    let object = compile(
        source,
        "forwarded-member-initializer.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the one-use helper should inline into the member initializer");

    let expected = [
        0x38, 0x00, 0x00, 0x00, // li r0,0
        0x2c, 0x04, 0x00, 0x01, // cmpwi r4,1
        0x90, 0x83, 0x00, 0x18, // stw r4,24(r3)
        0xd0, 0x23, 0x00, 0x04, // stfs f1,4(r3)
        0xd0, 0x43, 0x00, 0x08, // stfs f2,8(r3)
        0xd0, 0x63, 0x00, 0x10, // stfs f3,16(r3)
        0x90, 0x03, 0x00, 0x14, // stw r0,20(r3)
        0x98, 0x03, 0x00, 0x1c, // stb r0,28(r3)
        0x40, 0x82, 0x00, 0x0c, // bne false
        0xfc, 0x00, 0x08, 0x18, // frsp f0,f1
        0x48, 0x00, 0x00, 0x08, // b join
        0xfc, 0x00, 0x10, 0x18, // false: frsp f0,f2
        0xd0, 0x03, 0x00, 0x0c, // join: stfs f0,12(r3)
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}

#[test]
fn reuses_a_nested_pointer_base_across_float_binary_operands() {
    let source = br#"
        typedef struct Data {
            float low;
            float high;
        } Data;

        typedef struct Block {
            Data* data;
            float span;
        } Block;

        void calculate(Block* block) {
            block->span = block->data->high - block->data->low;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.optimization = mwcc_versions::Optimization::O4;
    let object = compile(
        source,
        "shared-nested-float-base.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_6,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("two nested float members should reuse their pointer-valued base");

    let expected = [
        0x80, 0x83, 0x00, 0x00, // lwz r4,0(r3)
        0xc0, 0x24, 0x00, 0x04, // lfs f1,4(r4)
        0xc0, 0x04, 0x00, 0x00, // lfs f0,0(r4)
        0xec, 0x01, 0x00, 0x28, // fsubs f0,f1,f0
        0xd0, 0x03, 0x00, 0x04, // stfs f0,4(r3)
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}
