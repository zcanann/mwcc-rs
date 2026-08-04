use crate::{compile, SourceLanguage};

#[test]
fn emits_legacy_bounded_heap_transactions_exactly() {
    let source = br#"
        typedef unsigned char u8;
        typedef signed int s32;
        typedef unsigned int u32;

        typedef struct ALHeap {
            u8* base;
            u8* current;
            s32 length;
            u32 count;
            u8* last;
        } ALHeap;

        void* heap_alloc(ALHeap* heap, s32 size) {
            int pad[4];
            do {} while (0);
            s32* size_address;
            size_address = &size;
            u32 rounded_size = (size + 32 - 1) & -32;
            if (!heap->base)
                return 0;
            u8* previous = heap->current;
            if (previous + rounded_size <= heap->base + heap->length)
                heap->current = previous + rounded_size;
            else
                return 0;
            heap->count++;
            heap->last = previous;
            return previous;
        }

        void heap_init(ALHeap* heap, u8* memory, s32 size) {
            int pad[2];
            do {} while (0);
            ALHeap** heap_address;
            s32 length;
            heap_address = &heap;
            heap->count = 0;
            if (!memory) {
                heap->length = 0;
                heap->current = 0;
                heap->last = 0;
            } else {
                length = size - ((u32)memory & 0x1f);
                heap->base = (u8*)(((u32)memory + 32 - 1) & -32);
                heap->current = heap->base;
                heap->length = length;
                heap->last = 0;
            }
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.optimization = mwcc_versions::Optimization::O4;
    flags.optimization_goal = mwcc_versions::OptimizationGoal::Size;
    flags.scheduling_model = mwcc_versions::SchedulingModel::PowerPc7400;
    flags.inline_enabled = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.use_lmw_stmw = true;
    flags.debug_info = false;
    let object = compile(
        source,
        "structured-heap-transactions.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        },
        Some(SourceLanguage::Cxx),
        None,
        false,
    )
    .expect("the bounded heap transactions should compile");

    let allocation = [
        0x94, 0x21, 0xff, 0xd8, 0x90, 0x81, 0x00, 0x0c, 0x80, 0xa3, 0x00, 0x00, 0x80, 0x81,
        0x00, 0x0c, 0x28, 0x05, 0x00, 0x00, 0x38, 0x04, 0x00, 0x1f, 0x54, 0x04, 0x00, 0x34,
        0x40, 0x82, 0x00, 0x0c, 0x38, 0x60, 0x00, 0x00, 0x48, 0x00, 0x00, 0x40, 0x80, 0xc3,
        0x00, 0x04, 0x80, 0x03, 0x00, 0x08, 0x7c, 0x86, 0x22, 0x14, 0x7c, 0x05, 0x02, 0x14,
        0x7c, 0x04, 0x00, 0x40, 0x41, 0x81, 0x00, 0x0c, 0x90, 0x83, 0x00, 0x04, 0x48, 0x00,
        0x00, 0x0c, 0x38, 0x60, 0x00, 0x00, 0x48, 0x00, 0x00, 0x18, 0x80, 0x83, 0x00, 0x0c,
        0x38, 0x04, 0x00, 0x01, 0x90, 0x03, 0x00, 0x0c, 0x90, 0xc3, 0x00, 0x10, 0x7c, 0xc3,
        0x33, 0x78, 0x38, 0x21, 0x00, 0x28, 0x4e, 0x80, 0x00, 0x20,
    ];
    let initialization = [
        0x94, 0x21, 0xff, 0xd8, 0x28, 0x04, 0x00, 0x00, 0x38, 0xc0, 0x00, 0x00, 0x90, 0x61,
        0x00, 0x08, 0x80, 0xe1, 0x00, 0x08, 0x90, 0xc7, 0x00, 0x0c, 0x40, 0x82, 0x00, 0x14,
        0x90, 0xc7, 0x00, 0x08, 0x90, 0xc7, 0x00, 0x04, 0x90, 0xc7, 0x00, 0x10, 0x48, 0x00,
        0x00, 0x28, 0x38, 0x64, 0x00, 0x1f, 0x54, 0x80, 0x06, 0xfe, 0x54, 0x63, 0x00, 0x34,
        0x7c, 0x00, 0x28, 0x50, 0x90, 0x67, 0x00, 0x00, 0x80, 0x67, 0x00, 0x00, 0x90, 0x67,
        0x00, 0x04, 0x90, 0x07, 0x00, 0x08, 0x90, 0xc7, 0x00, 0x10, 0x38, 0x21, 0x00, 0x28,
        0x4e, 0x80, 0x00, 0x20,
    ];
    for (name, expected) in [
        ("allocation", &allocation[..]),
        ("initialization", &initialization[..]),
    ] {
        assert!(
            object.windows(expected.len()).any(|bytes| bytes == expected),
            "missing exact {name} transaction",
        );
    }
}
