use crate::{compile, SourceLanguage};

fn compile_empty_virtual_destructor(build: mwcc_versions::CompilerBuild) -> Vec<u8> {
    let source = br#"
        class EmptyVirtual {
        public:
            virtual ~EmptyVirtual() {}
            virtual void run();
        };

        void EmptyVirtual::run() {}
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.rtti = false;
    compile(
        source,
        "empty-virtual-destructor.cpp",
        mwcc_versions::CompilerConfig { build, flags },
        Some(SourceLanguage::Cxx),
        None,
        false,
    )
    .expect("the empty virtual destructor should use its versioned ABI lowering")
}

#[test]
fn empty_virtual_destructor_tracks_the_4x_optimizer_transition() {
    let legacy = compile_empty_virtual_destructor(mwcc_versions::GC_2_6);
    let legacy_expected = [
        0x94, 0x21, 0xff, 0xf0, // stwu r1,-16(r1)
        0x7c, 0x08, 0x02, 0xa6, // mflr r0
        0x90, 0x01, 0x00, 0x14, // stw r0,20(r1)
        0x93, 0xe1, 0x00, 0x0c, // stw r31,12(r1)
        0x7c, 0x7f, 0x1b, 0x79, // mr. r31,r3
        0x41, 0x82, 0x00, 0x1c, // beq done
        0x3c, 0xa0, 0x00, 0x00, // lis r5,vtable@ha
        0x7c, 0x80, 0x07, 0x35, // extsh. r0,r4
        0x38, 0x05, 0x00, 0x00, // addi r0,r5,vtable@l
        0x90, 0x1f, 0x00, 0x00, // stw r0,0(r31)
        0x40, 0x81, 0x00, 0x08, // ble done
        0x48, 0x00, 0x00, 0x01, // bl operator delete
        0x80, 0x01, 0x00, 0x14, // lwz r0,20(r1)
        0x7f, 0xe3, 0xfb, 0x78, // mr r3,r31
        0x83, 0xe1, 0x00, 0x0c, // lwz r31,12(r1)
        0x7c, 0x08, 0x03, 0xa6, // mtlr r0
        0x38, 0x21, 0x00, 0x10, // addi r1,r1,16
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(legacy
        .windows(legacy_expected.len())
        .any(|bytes| bytes == legacy_expected));

    let modern = compile_empty_virtual_destructor(mwcc_versions::GC_3_0A3);
    let modern_expected = [
        0x94, 0x21, 0xff, 0xf0, // stwu r1,-16(r1)
        0x7c, 0x08, 0x02, 0xa6, // mflr r0
        0x2c, 0x03, 0x00, 0x00, // cmpwi r3,0
        0x90, 0x01, 0x00, 0x14, // stw r0,20(r1)
        0x93, 0xe1, 0x00, 0x0c, // stw r31,12(r1)
        0x7c, 0x7f, 0x1b, 0x78, // mr r31,r3
        0x41, 0x82, 0x00, 0x10, // beq done
        0x2c, 0x04, 0x00, 0x00, // cmpwi r4,0
        0x40, 0x81, 0x00, 0x08, // ble done
        0x48, 0x00, 0x00, 0x01, // bl operator delete
        0x7f, 0xe3, 0xfb, 0x78, // mr r3,r31
        0x83, 0xe1, 0x00, 0x0c, // lwz r31,12(r1)
        0x80, 0x01, 0x00, 0x14, // lwz r0,20(r1)
        0x7c, 0x08, 0x03, 0xa6, // mtlr r0
        0x38, 0x21, 0x00, 0x10, // addi r1,r1,16
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(modern
        .windows(modern_expected.len())
        .any(|bytes| bytes == modern_expected));
}
