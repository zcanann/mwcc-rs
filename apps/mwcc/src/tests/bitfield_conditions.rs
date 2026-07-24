use crate::{compile, SourceLanguage};

#[test]
fn compares_a_bitfield_value_with_one() {
    let source = br#"
        struct State {
            unsigned char b0 : 1;
            unsigned char b1 : 1;
            unsigned char b2 : 1;
            unsigned char selected : 1;
            unsigned char b4 : 1;
            unsigned char b5 : 1;
            unsigned char b6 : 1;
            unsigned char b7 : 1;
            int result;
        };

        void compiled(struct State* state) {
            if (state->selected == 1) {
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
        "bitfield-condition.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a bitfield comparison should compile");

    // lbz r0,0(r3); rlwinm r0,r0,28,31,31; cmplwi r0,1
    let expected = [
        0x88, 0x03, 0x00, 0x00, 0x54, 0x00, 0xe7, 0xfe, 0x28, 0x00, 0x00, 0x01,
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}
