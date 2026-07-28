use crate::{compile, SourceLanguage};

#[test]
fn emits_the_legacy_device_registration_event_switch() {
    let source = br#"
        typedef struct System {
            int pad[9];
            void* cpu;
        } System;
        typedef struct Device {
            void* host;
        } Device;

        extern int put8(Device*, unsigned int, char*);
        extern int put16(Device*, unsigned int, short*);
        extern int put32(Device*, unsigned int, int*);
        extern int put64(Device*, unsigned int, long long*);
        extern int get8(Device*, unsigned int, char*);
        extern int get16(Device*, unsigned int, short*);
        extern int get32(Device*, unsigned int, int*);
        extern int get64(Device*, unsigned int, long long*);
        extern int set_put(void*, void*, void*, void*, void*, void*);
        extern int set_get(void*, void*, void*, void*, void*, void*);

        int device_event(Device* device, int event, void* argument) {
            switch (event) {
                case 2:
                    device->host = argument;
                    break;
                case 0x1002:
                    if (!set_put(((System*)device->host)->cpu, argument,
                                 put8, put16, put32, put64)) return 0;
                    if (!set_get(((System*)device->host)->cpu, argument,
                                 get8, get16, get32, get64)) return 0;
                    break;
                case 0:
                case 1:
                case 3:
                case 0x1003:
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
        "device-registration-event-switch.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the device registration event switch should compile");

    let expected_prefix = [
        0x7c, 0x08, 0x02, 0xa6, 0x2c, 0x04, 0x00, 0x03, 0x90, 0x01, 0x00, 0x04, 0x94, 0x21, 0xff,
        0xe0, 0x93, 0xe1, 0x00, 0x1c, 0x3b, 0xe5, 0x00, 0x00, 0x93, 0xc1, 0x00, 0x18, 0x3b, 0xc3,
        0x00, 0x00, 0x41, 0x82, 0x00, 0xc4, 0x40, 0x80, 0x00, 0x18, 0x2c, 0x04, 0x00, 0x02, 0x40,
        0x80, 0x00, 0x28,
    ];
    assert!(object
        .windows(expected_prefix.len())
        .any(|bytes| bytes == expected_prefix));
}
