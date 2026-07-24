use crate::{compile, SourceLanguage};

#[test]
fn loads_stacked_parameters_before_populating_a_forwarded_aggregate() {
    let source = br#"
        typedef struct Params {
            unsigned short* image;
            unsigned short* palette;
            unsigned short width;
            unsigned short height;
            unsigned char format;
            unsigned char size;
            unsigned short table;
            unsigned short table_count;
            float x;
            float y;
            float x_scale;
            float y_scale;
            unsigned flags;
        } Params;

        extern void consume(Params*, unsigned**);

        void wrapper(
            unsigned** output,
            unsigned short* image,
            unsigned short* palette,
            unsigned short width,
            unsigned short height,
            unsigned char format,
            unsigned char size,
            unsigned short table,
            unsigned short table_count,
            float x,
            float y,
            float x_scale,
            float y_scale,
            unsigned flags
        ) {
            Params params;
            params.image = image;
            params.palette = palette;
            params.width = width;
            params.height = height;
            params.format = format;
            params.size = size;
            params.table = table;
            params.table_count = table_count;
            params.x = x;
            params.y = y;
            params.x_scale = x_scale;
            params.y_scale = y_scale;
            params.flags = flags;
            consume(&params, output);
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.emit_mwcats = false;
    flags.optimization = mwcc_versions::Optimization::O4;
    let object = compile(
        source,
        "aggregate-parameter-forwarder.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_2_6,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the aggregate parameter forwarder should compile");

    // Exact GC/2.6 output measured from Animal Crossing's wallpaper_draw.
    let expected = [
        0x94, 0x21, 0xff, 0xd0, // stwu r1,-48(r1)
        0x7c, 0x08, 0x02, 0xa6, // mflr r0
        0x90, 0x01, 0x00, 0x34, // stw r0,52(r1)
        0xa1, 0x61, 0x00, 0x3a, // lhz r11,58(r1)
        0x80, 0x01, 0x00, 0x3c, // lwz r0,60(r1)
        0x90, 0x81, 0x00, 0x08, // stw r4,8(r1)
        0x7c, 0x64, 0x1b, 0x78, // mr r4,r3
        0x38, 0x61, 0x00, 0x08, // addi r3,r1,8
        0x90, 0xa1, 0x00, 0x0c, // stw r5,12(r1)
        0xb0, 0xc1, 0x00, 0x10, // sth r6,16(r1)
        0xb0, 0xe1, 0x00, 0x12, // sth r7,18(r1)
        0x99, 0x01, 0x00, 0x14, // stb r8,20(r1)
        0x99, 0x21, 0x00, 0x15, // stb r9,21(r1)
        0xb1, 0x41, 0x00, 0x16, // sth r10,22(r1)
        0xb1, 0x61, 0x00, 0x18, // sth r11,24(r1)
        0xd0, 0x21, 0x00, 0x1c, // stfs f1,28(r1)
        0xd0, 0x41, 0x00, 0x20, // stfs f2,32(r1)
        0xd0, 0x61, 0x00, 0x24, // stfs f3,36(r1)
        0xd0, 0x81, 0x00, 0x28, // stfs f4,40(r1)
        0x90, 0x01, 0x00, 0x2c, // stw r0,44(r1)
        0x48, 0x00, 0x00, 0x01, // bl consume
        0x80, 0x01, 0x00, 0x34, // lwz r0,52(r1)
        0x7c, 0x08, 0x03, 0xa6, // mtlr r0
        0x38, 0x21, 0x00, 0x30, // addi r1,r1,48
        0x4e, 0x80, 0x00, 0x20, // blr
    ];
    assert!(object.windows(expected.len()).any(|bytes| bytes == expected));
}
