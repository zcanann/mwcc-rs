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

#[test]
fn reuses_the_first_argument_register_for_a_large_third_string() {
    let source = br#"
        extern void report(char*, int, char*);
        void compiled(int condition) {
            if (condition) {
                report("file.c", 299, "translation");
            }
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
        "mixed-string-line-arguments.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the large third string should reuse r3 while forming r5");

    let arguments_and_call = [
        0x3c, 0x60, 0x00, 0x00, // lis r3,third@ha
        0x38, 0xa3, 0x00, 0x00, // addi r5,r3,third@l
        0x38, 0x60, 0x00, 0x00, // li r3,first@sda21
        0x38, 0x80, 0x01, 0x2b, // li r4,299
        0x48, 0x00, 0x00, 0x01, // bl report
    ];
    assert!(object
        .windows(arguments_and_call.len())
        .any(|bytes| bytes == arguments_and_call));
}

#[test]
fn reconstructs_saved_siblings_after_a_nested_heap_call() {
    let source = br#"
        extern void release(void*);
        extern void* current_heap(void);
        extern void* allocate(unsigned, void*, int);
        void* replace(void* old, unsigned count) {
            release(old);
            return allocate(count * 16, current_heap(), 0);
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
        "nested-general-call-argument.c",
        config,
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("saved siblings should be reconstructed after the nested call");

    let nested_arguments = [
        0x48, 0x00, 0x00, 0x01, // bl current_heap
        0x38, 0x83, 0x00, 0x00, // addi r4,r3,0
        0x57, 0xe3, 0x20, 0x36, // slwi r3,r31,4
        0x38, 0xa0, 0x00, 0x00, // li r5,0
        0x48, 0x00, 0x00, 0x01, // bl allocate
    ];
    assert!(object
        .windows(nested_arguments.len())
        .any(|bytes| bytes == nested_arguments));
}

#[test]
fn reconstructs_an_aggregate_hidden_result_prefix_after_a_float_call() {
    let source = br#"
        struct Vec {
            float x;
            float y;
            float z;
            Vec& operator=(const Vec&);
            Vec operator*(float) const;
            float dot(const Vec&) const;
        };
        void scale(
            Vec& result,
            const Vec& right,
            float length
        ) {
            Vec left;
            result = left * (left.dot(right) / length);
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
    compile(
        source,
        "aggregate-hidden-result-float-tail.cpp",
        config,
        Some(SourceLanguage::Cxx),
        None,
        false,
    )
    .expect("the float call should precede the reloadable hidden-result and object addresses");
}
