use crate::{compile, SourceLanguage};

#[test]
fn initialized_call_accumulator_keeps_the_guard_before_its_updates() {
    let source = include_bytes!("../../../../canaries/1575_initialized_call_accumulator.c");
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    let object = compile(
        source,
        "initialized-call-accumulator.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_3,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("declaration-initialized accumulators must retain their entry values");

    // Measured GC/1.3 entry of retain_initial_error: preserve the initial
    // value, then call select_channel(4). Moving the accumulating calls ahead
    // of the guard changes observable call order and executes them on failure.
    let reference_entry = [
        0x94, 0x21, 0xff, 0xf0, // stwu r1,-16(r1)
        0x7c, 0x08, 0x02, 0xa6, // mflr r0
        0x90, 0x01, 0x00, 0x14, // stw r0,20(r1)
        0x93, 0xe1, 0x00, 0x0c, // stw r31,12(r1)
        0x7c, 0x7f, 0x1b, 0x78, // mr r31,r3
        0x38, 0x60, 0x00, 0x04, // li r3,4
        0x48, 0x00, 0x00, 0x01, // bl select_channel
        0x2c, 0x03, 0x00, 0x00, // cmpwi r3,0
    ];
    assert!(object
        .windows(reference_entry.len())
        .any(|bytes| bytes == reference_entry));
}
