use crate::{compile, SourceLanguage};

#[test]
fn emits_a_masked_word_store_jump_table_beside_file_data() {
    let source = br#"
        typedef struct Device {
            int host;
            int address;
        } Device;

        int descriptor[4] = { 1, 2, 3, 4 };

        int read_register(Device* device, unsigned int address, int* output) {
            address &= 0x1F;
            switch (address) {
                case 0x00:
                    *output = device->address;
                    break;
                case 0x04:
                    *output = 0;
                    break;
                case 0x10:
                    *output = 0;
                    break;
                case 0x18:
                    *output = 0;
                    break;
                default:
                    return 0;
            }
            return 1;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "masked-word-store-switch.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the masked register read and file data should compile together");

    let expected = [
        0x54, 0x80, 0x06, 0xfe, 0x28, 0x00, 0x00, 0x18, 0x41, 0x81, 0x00, 0x4c, 0x3c, 0x80, 0x00,
        0x00, 0x38, 0x84, 0x00, 0x00, 0x54, 0x00, 0x10, 0x3a, 0x7c, 0x04, 0x00, 0x2e, 0x7c, 0x09,
        0x03, 0xa6, 0x4e, 0x80, 0x04, 0x20, 0x80, 0x03, 0x00, 0x04, 0x90, 0x05, 0x00, 0x00, 0x48,
        0x00, 0x00, 0x30, 0x38, 0x00, 0x00, 0x00, 0x90, 0x05, 0x00, 0x00, 0x48, 0x00, 0x00, 0x24,
        0x38, 0x00, 0x00, 0x00, 0x90, 0x05, 0x00, 0x00, 0x48, 0x00, 0x00, 0x18, 0x38, 0x00, 0x00,
        0x00, 0x90, 0x05, 0x00, 0x00, 0x48, 0x00, 0x00, 0x0c, 0x38, 0x60, 0x00, 0x00, 0x4e, 0x80,
        0x00, 0x20, 0x38, 0x60, 0x00, 0x01, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}
