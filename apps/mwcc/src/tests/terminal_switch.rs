use crate::{compile, SourceLanguage};

#[test]
fn lowers_a_guarded_terminal_switch_with_nested_arms() {
    let source = br#"
        struct State {
            int enabled;
            int kind;
            int value;
            int limit;
            int alternate;
        };

        void compiled(struct State* state) {
            if (state->enabled != 1) {
                return;
            }
            switch (state->kind) {
            case 0:
                if (state->value >= state->limit) {
                    state->value = state->limit;
                    state->enabled = 0;
                }
                break;
            case 1:
                if (!state->alternate) {
                    if (state->value >= state->limit) {
                        state->alternate = 1;
                    }
                } else {
                    if (state->value <= 0) {
                        state->alternate = 0;
                    }
                }
                break;
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
        "terminal-switch.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a guarded terminal switch should lower through nested case arms");

    let guarded_dispatch = [
        0x80, 0x03, 0x00, 0x00, // lwz r0,0(r3)
        0x2c, 0x00, 0x00, 0x01, // cmpwi r0,1
        0x4c, 0x82, 0x00, 0x20, // bnelr
        0x80, 0x03, 0x00, 0x04, // lwz r0,4(r3)
    ];
    assert!(
        object
            .windows(guarded_dispatch.len())
            .any(|bytes| bytes == guarded_dispatch)
    );
}

#[test]
fn schedules_a_terminal_compound_float_clamp_as_one_region() {
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

        void compiled(struct Frame* frame) {
            if (frame->state != 1) {
                return;
            }
            switch (frame->kind) {
            case 0:
                if ((frame->current += frame->delta) >= frame->maximum - 1.0f) {
                    frame->current = frame->maximum - 1.0f;
                    frame->state = 0;
                }
                break;
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
        "terminal-float-clamp.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a terminal float clamp should compile as one scheduled region");

    let region = [
        0xc0, 0x63, 0x00, 0x04, // lfs f3,maximum(r3)
        0xc0, 0x40, 0x00, 0x00, // lfs f2,@1.0@sda21(0)
        0xc0, 0x23, 0x00, 0x0c, // lfs f1,current(r3)
        0xc0, 0x03, 0x00, 0x10, // lfs f0,delta(r3)
        0xec, 0x43, 0x10, 0x28, // fsubs f2,f3,f2
        0xec, 0x01, 0x00, 0x2a, // fadds f0,f1,f0
        0xfc, 0x00, 0x10, 0x40, // fcmpo cr0,f0,f2
        0xd0, 0x03, 0x00, 0x0c, // stfs f0,current(r3)
        0x4c, 0x41, 0x13, 0x82, // cror eq,gt,eq
        0x4c, 0x82, 0x00, 0x20, // bnelr
        0x38, 0x00, 0x00, 0x00, // li r0,0
        0xd0, 0x43, 0x00, 0x0c, // stfs f2,current(r3)
        0x90, 0x03, 0x00, 0x14, // stw r0,state(r3)
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object.windows(region.len()).any(|bytes| bytes == region));
}
