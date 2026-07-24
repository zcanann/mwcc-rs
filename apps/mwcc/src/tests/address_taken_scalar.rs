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
