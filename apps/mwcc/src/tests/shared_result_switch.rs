use crate::{compile, SourceLanguage};

#[test]
fn shares_one_result_block_across_sparse_empty_case_arms() {
    let source = br#"
        int compiled(void* object, unsigned int address, int* data) {
            switch (address & 0xF) {
                case 0:
                case 8:
                case 12:
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
        "shared-result-switch.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("a sparse shared-result switch should compile");

    let expected = [
        0x54, 0x80, 0x07, 0x3e, // clrlwi r0,r4,28
        0x2c, 0x00, 0x00, 0x08, // cmpwi r0,8
        0x41, 0x82, 0x00, 0x24, // beq success
        0x40, 0x80, 0x00, 0x10, // bge upper
        0x2c, 0x00, 0x00, 0x00, // cmpwi r0,0
        0x41, 0x82, 0x00, 0x18, // beq success
        0x48, 0x00, 0x00, 0x0c, // b default
        0x2c, 0x00, 0x00, 0x0c, // cmpwi r0,12
        0x41, 0x82, 0x00, 0x0c, // beq success
        0x38, 0x60, 0x00, 0x00, // default: li r3,0
        0x4e, 0x80, 0x00, 0x20, // blr
        0x38, 0x60, 0x00, 0x01, // success: li r3,1
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}

#[test]
fn uses_a_comparison_tree_for_three_sparse_return_arms() {
    let source = br#"
        int compiled(int value) {
            switch (value) {
                case 0: return 10;
                case 8: return 20;
                case 12: return 30;
                default: return 40;
            }
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "sparse-return-switch.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_1,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("three sparse return arms should use the comparison tree");

    let expected = [
        0x2c, 0x03, 0x00, 0x08, 0x41, 0x82, 0x00, 0x28, 0x40, 0x80, 0x00, 0x10, 0x2c, 0x03, 0x00,
        0x00, 0x41, 0x82, 0x00, 0x14, 0x48, 0x00, 0x00, 0x28, 0x2c, 0x03, 0x00, 0x0c, 0x41, 0x82,
        0x00, 0x18, 0x48, 0x00, 0x00, 0x1c, 0x38, 0x60, 0x00, 0x0a, 0x4e, 0x80, 0x00, 0x20, 0x38,
        0x60, 0x00, 0x14, 0x4e, 0x80, 0x00, 0x20, 0x38, 0x60, 0x00, 0x1e, 0x4e, 0x80, 0x00, 0x20,
        0x38, 0x60, 0x00, 0x28, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}
