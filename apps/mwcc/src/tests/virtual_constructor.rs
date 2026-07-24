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

#[test]
fn file_ipa_erases_the_overwritten_base_vptr_and_reuses_zero() {
    let source = br#"
        namespace Scene {
            class Base {
            public:
                Base() {}
                virtual ~Base();
                virtual void run();
            };
            class Derived : public Base {
            public:
                void* first;
                void* second;
                Derived();
                virtual ~Derived();
                virtual void run();
            };
            Base::~Base() {}
            Derived::Derived() : first(), second() {}
            Derived::~Derived() {}
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.rtti = false;
    flags.optimization = mwcc_versions::Optimization::O4;
    flags.ipa_file = true;
    flags.inline_enabled = true;
    let object = compile(
        source,
        "ipa-virtual-constructor.cpp",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_3_0A3,
            flags,
        },
        Some(SourceLanguage::Cxx),
        None,
        false,
    )
    .expect("the file-IPA constructor should compile");

    let expected = [
        0x3c, 0x80, 0x00, 0x00, // lis r4,vtable@ha
        0x38, 0x00, 0x00, 0x00, // li r0,0
        0x38, 0x84, 0x00, 0x00, // addi r4,r4,vtable@l
        0x90, 0x03, 0x00, 0x04, // stw r0,4(r3)
        0x90, 0x83, 0x00, 0x00, // stw r4,0(r3)
        0x90, 0x03, 0x00, 0x08, // stw r0,8(r3)
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}

#[test]
fn long_string_member_store_completes_its_address_from_a_nonzero_base() {
    let source = br#"
        struct Holder {
            const char* value;
        };
        void set(Holder* holder) {
            holder->value = "a string longer than small data";
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.rtti = false;
    let object = compile(
        source,
        "string-member-store.cpp",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        },
        Some(SourceLanguage::Cxx),
        None,
        false,
    )
    .expect("the absolute string address should compile");

    let expected = [
        0x3c, 0x80, 0x00, 0x00, // lis r4,string@ha
        0x38, 0x04, 0x00, 0x00, // addi r0,r4,string@l
        0x90, 0x03, 0x00, 0x00, // stw r0,0(r3)
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}
