use crate::{compile, SourceLanguage};

#[test]
fn schedules_indexed_addresses_and_a_float_product_like_mwcc() {
    let source = br#"
        typedef struct Values {
            void* handle;
            float scale;
        } Values;
        typedef struct Vec3 {
            float x;
            float y;
            float z;
        } Vec3;
        extern void consume_float(void* handle, float value);
        extern void consume_addresses(void* handle, Vec3* second, Vec3* first);
        void float_schedule(Values* values, Vec3* vectors) {
            consume_float(values->handle, values->scale * vectors[2].x);
        }
        void address_schedule(Values* values, Vec3* vectors) {
            consume_addresses(values->handle, &vectors[1], &vectors[0]);
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let config = mwcc_versions::CompilerConfig {
        build: mwcc_versions::GC_1_2_5N,
        flags,
    };
    let object = compile(
        source,
        "call-argument-schedules.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the measured argument schedules should compile");

    // Exact GC/1.2.5n output measured with mwcceppc. The independent handle
    // load occupies the two lfs instructions' latency slot before fmuls.
    let float_schedule = [
        0x7c, 0x08, 0x02, 0xa6, 0x90, 0x01, 0x00, 0x04, 0x94, 0x21, 0xff, 0xf8,
        0xc0, 0x23, 0x00, 0x04, 0xc0, 0x04, 0x00, 0x18, 0x80, 0x63, 0x00, 0x00,
        0xec, 0x21, 0x00, 0x32, 0x48, 0x00, 0x00, 0x01, 0x80, 0x01, 0x00, 0x0c,
        0x38, 0x21, 0x00, 0x08, 0x7c, 0x08, 0x03, 0xa6, 0x4e, 0x80, 0x00, 0x20,
    ];
    // The zero-index address is evaluated first and fills the mflr latency
    // slot; the index-one address follows before the member load.
    let address_schedule = [
        0x7c, 0x08, 0x02, 0xa6, 0x38, 0xa4, 0x00, 0x00, 0x90, 0x01, 0x00, 0x04,
        0x38, 0x84, 0x00, 0x0c, 0x94, 0x21, 0xff, 0xf8, 0x80, 0x63, 0x00, 0x00,
        0x48, 0x00, 0x00, 0x01, 0x80, 0x01, 0x00, 0x0c, 0x38, 0x21, 0x00, 0x08,
        0x7c, 0x08, 0x03, 0xa6, 0x4e, 0x80, 0x00, 0x20,
    ];
    assert!(object
        .windows(float_schedule.len())
        .any(|bytes| bytes == float_schedule));
    assert!(object
        .windows(address_schedule.len())
        .any(|bytes| bytes == address_schedule));
}
