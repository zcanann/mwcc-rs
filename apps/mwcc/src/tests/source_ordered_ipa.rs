use crate::{compile, SourceLanguage};

#[test]
fn leaves_a_forward_defined_pointer_walker_out_of_line() {
    let source = br#"
        typedef void (*VoidFunction)(void);
        extern VoidFunction _ctors[];

        extern "C" {
        void __init_cpp(void);

        void __init_user(void) {
            __init_cpp();
        }

        void __init_cpp(void) {
            VoidFunction* constructor;
            for (constructor = _ctors; *constructor; constructor++) {
                (*constructor)();
            }
        }
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "source-ordered-ipa.cpp",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        },
        Some(SourceLanguage::Cxx),
        None,
        false,
    )
    .expect("the forward-defined constructor walker should compile");

    // Exact GC/1.2.5n output: file IPA has not seen the later definition yet,
    // so the wrapper remains a normal non-tail call.
    let expected = [
        0x7c, 0x08, 0x02, 0xa6, // mflr r0
        0x90, 0x01, 0x00, 0x04, // stw r0,4(r1)
        0x94, 0x21, 0xff, 0xf8, // stwu r1,-8(r1)
        0x48, 0x00, 0x00, 0x01, // bl __init_cpp
        0x80, 0x01, 0x00, 0x0c, // lwz r0,12(r1)
        0x38, 0x21, 0x00, 0x08, // addi r1,r1,8
        0x7c, 0x08, 0x03, 0xa6, // mtlr r0
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}
