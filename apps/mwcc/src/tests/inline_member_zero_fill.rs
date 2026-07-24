use crate::{compile, SourceLanguage};

#[test]
fn reuses_a_float_zero_after_loading_an_inline_local_from_a_member() {
    let source = br#"
        struct State { float x; float y; };
        struct Owner { char pad[44]; struct State* state; };

        inline void reset(struct Owner* owner) {
            struct State* state;
            state = owner->state;
            state->x = 0;
            state->y = 0;
        }

        void compiled(struct Owner* owner) {
            reset(owner);
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
        "inline-member-zero-fill.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("an inline member-loaded zero fill should compile");

    // lwz r3,44(r3); lfs f0,@zero; stfs f0,0(r3); stfs f0,4(r3); blr
    let expected = [
        0x80, 0x63, 0x00, 0x2c, 0xc0, 0x00, 0x00, 0x00, 0xd0, 0x03, 0x00, 0x00,
        0xd0, 0x03, 0x00, 0x04, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}
