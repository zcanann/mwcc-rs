use crate::{compile, SourceLanguage};

#[test]
fn keeps_a_call_discriminator_in_r3_through_single_call_arms() {
    let source = br#"
        int get_count(void);
        int format(char*, const char*, ...);
        char* copy(char*, const char*);

        char* render(void) {
            static char buffer[12];
            int count = get_count();
            if (count > 0) {
                format(buffer, "%c", 'A' + (count - 1));
            } else {
                copy(buffer, "A or B");
            }
            return buffer;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.string_literals_read_only = true;
    flags.string_literals_packed = true;
    let object = compile(
        source,
        "call-result-if-else.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_0P1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a single-call result diamond should compile");

    // stwu; mflr; stw LR; bl get_count; cmpwi r3,0; ble else
    let entry = [
        0x94, 0x21, 0xff, 0xf0, 0x7c, 0x08, 0x02, 0xa6, 0x90, 0x01, 0x00, 0x14,
        0x48, 0x00, 0x00, 0x01, 0x2c, 0x03, 0x00, 0x00, 0x40, 0x81, 0x00, 0x28,
    ];
    assert!(object.windows(entry.len()).any(|bytes| bytes == entry));

    // At the join, reload LR before materializing the stable static-buffer
    // address, then tear down the 16-byte linkage frame.
    let exit = [
        0x80, 0x01, 0x00, 0x14, 0x3c, 0x60, 0x00, 0x00, 0x38, 0x63, 0x00, 0x00,
        0x7c, 0x08, 0x03, 0xa6, 0x38, 0x21, 0x00, 0x10, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(object.windows(exit.len()).any(|bytes| bytes == exit));
}
