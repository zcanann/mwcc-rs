use super::elf_object::function_bytes;
use crate::{compile, SourceLanguage};

#[test]
fn initializes_an_address_taken_scalar_in_its_frame_slot() {
    let source = br#"
        extern void consume(int*);

        void bridge(int value) {
            int slot = value;
            consume(&slot);
            consume(&slot);
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "address-taken-scalar.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_6,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the initialized scalar frame should compile");

    // Exact GC/2.6 output measured from mwcceppc.
    let expected = [
        0x94, 0x21, 0xff, 0xf0, // stwu r1,-16(r1)
        0x7c, 0x08, 0x02, 0xa6, // mflr r0
        0x90, 0x01, 0x00, 0x14, // stw r0,20(r1)
        0x90, 0x61, 0x00, 0x08, // stw r3,8(r1)
        0x38, 0x61, 0x00, 0x08, // addi r3,r1,8
        0x48, 0x00, 0x00, 0x01, // bl consume
        0x38, 0x61, 0x00, 0x08, // addi r3,r1,8
        0x48, 0x00, 0x00, 0x01, // bl consume
        0x80, 0x01, 0x00, 0x14, // lwz r0,20(r1)
        0x7c, 0x08, 0x03, 0xa6, // mtlr r0
        0x38, 0x21, 0x00, 0x10, // addi r1,r1,16
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}

#[test]
fn stores_a_later_assignment_to_an_address_taken_scalar() {
    let source = br#"
        extern void consume(int*);

        void bridge(int value) {
            int slot;
            slot = value;
            consume(&slot);
            consume(&slot);
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "assigned-address-taken-scalar.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_6,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the assigned scalar frame should compile");

    // The assignment must initialize the frame slot before its address escapes.
    let expected = [
        0x90, 0x61, 0x00, 0x08, // stw r3,8(r1)
        0x38, 0x61, 0x00, 0x08, // addi r3,r1,8
        0x48, 0x00, 0x00, 0x01, // bl consume
    ];
    assert!(
        object
            .windows(expected.len())
            .any(|bytes| bytes == expected),
        "the assigned value was not stored before the frame address escaped"
    );
}

#[test]
fn forwards_a_just_published_frame_scalar_to_its_next_use() {
    let source = br#"
        extern int produce(void);
        extern void consume_value(int);
        extern void consume_address(int*);

        void bridge(void) {
            int slot;
            slot = produce();
            consume_value(slot);
            consume_address(&slot);
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "forwarded-address-taken-scalar.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_6,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the published frame value should compile");

    let expected = [
        0x48, 0x00, 0x00, 0x01, // bl produce
        0x90, 0x61, 0x00, 0x08, // stw r3,8(r1)
        0x48, 0x00, 0x00, 0x01, // bl consume_value (no intervening lwz)
        0x38, 0x61, 0x00, 0x08, // addi r3,r1,8
        0x48, 0x00, 0x00, 0x01, // bl consume_address
    ];
    assert!(
        object
            .windows(expected.len())
            .any(|bytes| bytes == expected),
        "the immediately reused value was reloaded from its frame slot"
    );
}

#[test]
fn reloads_an_address_taken_scalar_for_a_pointer_store() {
    let source = br#"
        extern void mutate(int*);

        void bridge(int* output, int value) {
            int slot = value;
            mutate(&slot);
            *output = slot;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "stored-address-taken-scalar.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_6,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the frame scalar pointer store should compile");

    assert!(
        !object
            .windows(b"slot\0".len())
            .any(|bytes| bytes == b"slot\0"),
        "the frame scalar was misclassified as an external symbol"
    );
}

#[test]
fn reloads_an_address_taken_parameter_after_its_pointer_escapes() {
    let source = br#"
        extern int rewrite(unsigned*);
        extern int consume(int);

        int bridge(int size) {
            if (!rewrite((unsigned*)&size)) {
                return 0;
            }
            return consume(size);
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "escaped-address-taken-parameter.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the address-taken parameter should acquire a frame slot");

    // The incoming value initializes the object before its address escapes.
    let publication = [
        0x90, 0x61, 0x00, 0x08, // stw r3,8(r1)
        0x38, 0x61, 0x00, 0x08, // addi r3,r1,8
        0x48, 0x00, 0x00, 0x01, // bl rewrite
    ];
    assert!(object
        .windows(publication.len())
        .any(|bytes| bytes == publication));

    // rewrite may replace size through the escaped pointer. Passing the stale
    // incoming r3 to consume would be a miscompile; the frame object must win.
    let reload = [
        0x80, 0x61, 0x00, 0x08, // lwz r3,8(r1)
        0x48, 0x00, 0x00, 0x01, // bl consume
    ];
    assert!(object.windows(reload.len()).any(|bytes| bytes == reload));
}

#[test]
fn matches_gc_1_1_addressable_parameter_call_scheduling() {
    let source = br#"
        struct System { int pad[9]; void* objects[4]; };
        struct Device { struct System* host; };
        extern int fetch(void*, void**, int, unsigned*);
        extern int consume(int, void*, int);

        int bridge(struct Device* device, int offset, int selector, int size) {
            void* target;
            if (!fetch(device->host->objects[2], &target, offset, (unsigned*)&size)) {
                return 0;
            }
            if (!consume(selector & 0x7fff, target, size)) {
                return 0;
            }
            return 1;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "addressable-parameter-call-schedule.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the addressable-parameter call schedule should compile");

    // Exact GC/1.1 output measured from mwcceppc.
    let expected = [
        0x7c, 0x08, 0x02, 0xa6, // mflr r0
        0x90, 0x01, 0x00, 0x04, // stw r0,4(r1)
        0x94, 0x21, 0xff, 0xd8, // stwu r1,-40(r1)
        0x93, 0xe1, 0x00, 0x24, // stw r31,36(r1)
        0x7c, 0xbf, 0x2b, 0x78, // mr r31,r5
        0x38, 0xa4, 0x00, 0x00, // addi r5,r4,0
        0x90, 0xc1, 0x00, 0x14, // stw r6,20(r1)
        0x38, 0x81, 0x00, 0x18, // addi r4,r1,24
        0x38, 0xc1, 0x00, 0x14, // addi r6,r1,20
        0x80, 0x63, 0x00, 0x00, // lwz r3,0(r3)
        0x80, 0x63, 0x00, 0x2c, // lwz r3,44(r3)
        0x48, 0x00, 0x00, 0x01, // bl fetch
        0x2c, 0x03, 0x00, 0x00, // cmpwi r3,0
        0x40, 0x82, 0x00, 0x0c, // bne
        0x38, 0x60, 0x00, 0x00, // li r3,0
        0x48, 0x00, 0x00, 0x28, // b
        0x80, 0x81, 0x00, 0x18, // lwz r4,24(r1)
        0x57, 0xe3, 0x04, 0x7e, // clrlwi r3,r31,17
        0x80, 0xa1, 0x00, 0x14, // lwz r5,20(r1)
        0x48, 0x00, 0x00, 0x01, // bl consume
        0x2c, 0x03, 0x00, 0x00, // cmpwi r3,0
        0x40, 0x82, 0x00, 0x0c, // bne
        0x38, 0x60, 0x00, 0x00, // li r3,0
        0x48, 0x00, 0x00, 0x08, // b
        0x38, 0x60, 0x00, 0x01, // li r3,1
        0x80, 0x01, 0x00, 0x2c, // lwz r0,44(r1)
        0x83, 0xe1, 0x00, 0x24, // lwz r31,36(r1)
        0x38, 0x21, 0x00, 0x28, // addi r1,r1,40
        0x7c, 0x08, 0x03, 0xa6, // mtlr r0
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert_eq!(function_bytes(&object, "bridge"), expected);
}
