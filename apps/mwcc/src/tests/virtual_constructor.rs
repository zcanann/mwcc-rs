use crate::{compile, SourceLanguage};

#[test]
fn lowers_pointer_initialization_and_this_publication_as_one_constructor_transaction() {
    let source = br#"
        class Interface {
        public:
            static Interface* instance;
            Interface();
            virtual void run() = 0;
            void* heap;
        };

        Interface* Interface::instance;

        Interface::Interface() {
            heap = 0;
            instance = this;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.rtti = false;
    let object = compile(
        source,
        "virtual-constructor.cpp",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_7,
            flags,
        },
        Some(SourceLanguage::Cxx),
        None,
        false,
    )
    .expect("the leaf virtual constructor should compile");

    let expected = [
        0x3c, 0x80, 0x00, 0x00, // lis r4,vtable@ha
        0x38, 0x04, 0x00, 0x00, // addi r0,r4,vtable@l
        0x90, 0x03, 0x00, 0x00, // stw r0,0(r3)
        0x38, 0x00, 0x00, 0x00, // li r0,0
        0x90, 0x03, 0x00, 0x04, // stw r0,4(r3)
        0x90, 0x60, 0x00, 0x00, // stw r3,instance@sda21
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}
