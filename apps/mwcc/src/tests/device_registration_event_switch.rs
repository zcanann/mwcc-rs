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

#[test]
fn initializes_an_extra_zero_member_before_registering_callbacks() {
    let source = br#"
        typedef struct System {
            int pad[9];
            void* cpu;
        } System;
        typedef struct Device {
            void* host;
            int index;
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
            Device* self = (Device*)device;
            switch (event) {
                case 2:
                    self->host = argument;
                    self->index = 0;
                    break;
                case 0x1002:
                    if (!set_put(((System*)self->host)->cpu, argument,
                                 put8, put16, put32, put64)) return 0;
                    if (!set_get(((System*)self->host)->cpu, argument,
                                 get8, get16, get32, get64)) return 0;
                case 0:
                case 1:
                case 3:
                    break;
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
        "device-registration-event-zero-member.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the extended registration-event initializer should compile");

    let expected_prefix = [
        0x7c, 0x08, 0x02, 0xa6, // mflr r0
        0x2c, 0x04, 0x00, 0x03, // cmpwi r4,3
        0x90, 0x01, 0x00, 0x04, // stw r0,4(r1)
        0x94, 0x21, 0xff, 0xe0, // stwu r1,-32(r1)
        0x93, 0xe1, 0x00, 0x1c, // stw r31,28(r1)
        0x3b, 0xe5, 0x00, 0x00, // mr r31,r5
        0x93, 0xc1, 0x00, 0x18, // stw r30,24(r1)
        0x3b, 0xc3, 0x00, 0x00, // mr r30,r3
        0x41, 0x82, 0x00, 0xcc, // beq success
        0x40, 0x80, 0x00, 0x18, // bge upper
        0x2c, 0x04, 0x00, 0x02, // cmpwi r4,2
        0x40, 0x80, 0x00, 0x28, // bge initialize
        0x2c, 0x04, 0x00, 0x00, // cmpwi r4,0
        0x40, 0x80, 0x00, 0xb8, // bge success
        0x48, 0x00, 0x00, 0xac, // b failure
        0x2c, 0x04, 0x10, 0x03, // upper: cmpwi r4,0x1003
        0x41, 0x82, 0x00, 0xac, // beq success
        0x40, 0x80, 0x00, 0xa0, // bge failure
        0x2c, 0x04, 0x10, 0x02, // cmpwi r4,0x1002
        0x40, 0x80, 0x00, 0x18, // bge register
        0x48, 0x00, 0x00, 0x94, // b failure
        0x93, 0xfe, 0x00, 0x00, // initialize: stw r31,host(r30)
        0x38, 0x00, 0x00, 0x00, // li r0,0
        0x90, 0x1e, 0x00, 0x04, // stw r0,index(r30)
        0x48, 0x00, 0x00, 0x8c, // b success
    ];
    assert!(object
        .windows(expected_prefix.len())
        .any(|bytes| bytes == expected_prefix));
}
