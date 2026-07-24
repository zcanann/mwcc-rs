use crate::{compile, SourceLanguage};

#[test]
fn batches_parameter_member_setup_with_shared_zero_and_early_bounds() {
    let source = br#"
        typedef struct Setup {
            unsigned short width;
            unsigned short height;
            void* framebuffer;
            void* zbuffer;
            unsigned short ulx;
            unsigned short uly;
            unsigned short lrx;
            unsigned short lry;
        } Setup;

        void setup(Setup* output, unsigned width, unsigned height,
                   void* framebuffer, void* zbuffer) {
            output->width = width;
            output->height = height;
            output->framebuffer = framebuffer;
            output->zbuffer = zbuffer;
            output->ulx = 0;
            output->uly = 0;
            output->lrx = width - 1;
            output->lry = height - 1;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.emit_mwcats = false;
    flags.optimization = mwcc_versions::Optimization::O4;
    let object = compile(
        source,
        "member-setup.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_6,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the parameter member setup should compile");

    let expected = [
        0xb0, 0x83, 0x00, 0x00, // sth r4,0(r3)
        0x39, 0x00, 0x00, 0x00, // li r8,0
        0x38, 0x84, 0xff, 0xff, // addi r4,r4,-1
        0x38, 0x05, 0xff, 0xff, // addi r0,r5,-1
        0xb0, 0xa3, 0x00, 0x02, // sth r5,2(r3)
        0x90, 0xc3, 0x00, 0x04, // stw r6,4(r3)
        0x90, 0xe3, 0x00, 0x08, // stw r7,8(r3)
        0xb1, 0x03, 0x00, 0x0c, // sth r8,12(r3)
        0xb1, 0x03, 0x00, 0x0e, // sth r8,14(r3)
        0xb0, 0x83, 0x00, 0x10, // sth r4,16(r3)
        0xb0, 0x03, 0x00, 0x12, // sth r0,18(r3)
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}
