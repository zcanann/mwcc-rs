use crate::{compile, SourceLanguage};

#[test]
fn emits_the_legacy_masked_transfer_command_switch() {
    let source = br#"
        typedef struct Ram { int words[3]; } Ram;
        typedef struct Pif { int words[12]; } Pif;
        typedef struct System {
            int pad[9];
            void* objects[25];
        } System;
        typedef struct Serial {
            void* host;
            int address;
        } Serial;

        extern int ramGetBuffer(Ram*, void**, unsigned int, unsigned int*);
        extern int pifGetData(Pif*, void*);
        extern int pifSetData(Pif*, void*);
        extern int xlObjectEvent(void*, int, void*);

        int serialPut32(Serial* serial, unsigned int address, int* data) {
            unsigned int size;
            void* buffer;

            address &= 0x1F;
            switch (address) {
                case 0:
                    serial->address = *data;
                    break;
                case 4:
                    size = 0x40;
                    if (!ramGetBuffer((Ram*)((System*)serial->host)->objects[2],
                                      &buffer, serial->address, &size)) return 0;
                    if (!pifGetData((Pif*)((System*)serial->host)->objects[1],
                                    buffer)) return 0;
                    xlObjectEvent(serial->host, 0x1000, (void*)6);
                    break;
                case 16:
                    size = 0x40;
                    if (!ramGetBuffer((Ram*)((System*)serial->host)->objects[2],
                                      &buffer, serial->address, &size)) return 0;
                    if (!pifSetData((Pif*)((System*)serial->host)->objects[1],
                                    buffer)) return 0;
                    xlObjectEvent(serial->host, 0x1000, (void*)6);
                    break;
                case 24:
                    xlObjectEvent(serial->host, 0x1001, (void*)6);
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
        "masked-transfer-command-switch.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the masked transfer command switch should compile");

    let expected_prefix = [
        0x7c, 0x08, 0x02, 0xa6, 0x90, 0x01, 0x00, 0x04, 0x54, 0x80, 0x06, 0xfe, 0x28, 0x00, 0x00,
        0x18, 0x94, 0x21, 0xff, 0xd8, 0x93, 0xe1, 0x00, 0x24, 0x3b, 0xe3, 0x00, 0x00, 0x41, 0x81,
        0x01, 0x04, 0x3c, 0x60, 0x00, 0x00, 0x38, 0x63, 0x00, 0x00, 0x54, 0x00, 0x10, 0x3a, 0x7c,
        0x03, 0x00, 0x2e, 0x7c, 0x09, 0x03, 0xa6, 0x4e, 0x80, 0x04, 0x20,
    ];
    assert!(object
        .windows(expected_prefix.len())
        .any(|bytes| bytes == expected_prefix));
}
