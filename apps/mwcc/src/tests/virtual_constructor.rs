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
fn stores_an_inlined_placement_new_result_directly_from_r3() {
    let source = br#"
        typedef unsigned long size_t;
        class JKRHeap {};
        void* operator new(size_t, JKRHeap*, int);

        struct Node {
            int value;
            Node() : value(1) {}
        };
        struct Holder {
            Node* node;
            void init(JKRHeap* heap);
        };
        void Holder::init(JKRHeap* heap) {
            node = new (heap, 0) Node;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.rtti = false;
    flags.optimization = mwcc_versions::Optimization::O4;
    flags.optimization_goal = mwcc_versions::OptimizationGoal::Size;
    flags.inline_enabled = true;
    let object = compile(
        source,
        "placement-new-member-store.cpp",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_6,
            flags,
        },
        Some(SourceLanguage::Cxx),
        None,
        false,
    )
    .expect("the inlined placement-new member store should compile");

    let expected = [
        0x94, 0x21, 0xff, 0xf0, // stwu r1,-16(r1)
        0x7c, 0x08, 0x02, 0xa6, // mflr r0
        0x90, 0x01, 0x00, 0x14, // stw r0,20(r1)
        0x93, 0xe1, 0x00, 0x0c, // stw r31,12(r1)
        0x7c, 0x7f, 0x1b, 0x78, // mr r31,r3
        0x38, 0x60, 0x00, 0x04, // li r3,4
        0x38, 0xa0, 0x00, 0x00, // li r5,0
        0x48, 0x00, 0x00, 0x01, // bl operator new (REL24)
        0x7c, 0x63, 0x1b, 0x79, // or. r3,r3,r3
        0x41, 0x82, 0x00, 0x0c, // beq construction_done
        0x38, 0x00, 0x00, 0x01, // li r0,1
        0x90, 0x03, 0x00, 0x00, // stw r0,0(r3)
        0x90, 0x7f, 0x00, 0x00, // stw r3,0(r31)
        0x80, 0x01, 0x00, 0x14, // lwz r0,20(r1)
        0x83, 0xe1, 0x00, 0x0c, // lwz r31,12(r1)
        0x7c, 0x08, 0x03, 0xa6, // mtlr r0
        0x38, 0x21, 0x00, 0x10, // addi r1,r1,16
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
