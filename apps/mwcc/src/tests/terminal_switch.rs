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
