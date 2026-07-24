use crate::{compile, SourceLanguage};

#[test]
fn preserves_saved_float_initializers_and_entry_alias_lifetimes() {
    let source = br#"
        typedef struct Pair { float x; float y; } Pair;
        typedef struct State {
            char pad0[224];
            int ground;
            char pad1[1888];
            Pair pair;
        } State;
        extern float produce(float, float);
        extern void report(void);
        extern void consume(State*, int, float);

        void through_alias(State* state) {
            Pair* pair = &state->pair;
            float value = -produce(pair->x, pair->y);
            if (state->ground != 0) report();
            consume(state, 0, value);
        }

        void direct(State* state) {
            float value = -produce(state->pair.x, state->pair.y);
            if (state->ground != 0) report();
            consume(state, 0, value);
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
        "saved-float-initializer.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("call-initialized saved floats should compile");

    // Exact GC/1.2.5n output measured with mwcceppc. A source pointer alias
    // switches argument two to the saved GPR; direct member reads retain r3.
    // Both forms switch later reads to r31 after `produce` clobbers the entry
    // alias and fill the result latency slot before negating into f31.
    let through_alias = expected_code(0x5f);
    let direct = expected_code(0x43);
    assert!(object
        .windows(through_alias.len())
        .any(|bytes| bytes == through_alias));
    assert!(object.windows(direct.len()).any(|bytes| bytes == direct));
}

fn expected_code(second_float_base: u8) -> Vec<u8> {
    vec![
        0x7c, 0x08, 0x02, 0xa6, 0x90, 0x01, 0x00, 0x04, 0x94, 0x21, 0xff, 0xe0,
        0xdb, 0xe1, 0x00, 0x18, 0x93, 0xe1, 0x00, 0x14, 0x7c, 0x7f, 0x1b, 0x78,
        0xc0, 0x23, 0x08, 0x44, 0xc0, second_float_base, 0x08, 0x48, 0x48, 0x00,
        0x00, 0x01, 0x80, 0x1f, 0x00, 0xe0, 0xff, 0xe0, 0x08, 0x50, 0x2c, 0x00,
        0x00, 0x00, 0x41, 0x82, 0x00, 0x08, 0x48, 0x00, 0x00, 0x01, 0xfc, 0x20,
        0xf8, 0x90, 0x38, 0x7f, 0x00, 0x00, 0x38, 0x80, 0x00, 0x00, 0x48, 0x00,
        0x00, 0x01, 0x80, 0x01, 0x00, 0x24, 0xcb, 0xe1, 0x00, 0x18, 0x83, 0xe1,
        0x00, 0x14, 0x38, 0x21, 0x00, 0x20, 0x7c, 0x08, 0x03, 0xa6, 0x4e, 0x80,
        0x00, 0x20,
    ]
}
