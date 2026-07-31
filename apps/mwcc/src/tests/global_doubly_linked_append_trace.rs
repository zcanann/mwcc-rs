use crate::{compile, SourceLanguage};

#[test]
fn emits_the_revolution_traced_global_queue_append() {
    let source = br#"
        typedef unsigned int u32;

        typedef struct Task {
            u32 state;
            u32 priority;
            u32 flags;
            u32 reserved[11];
            struct Task* next;
            struct Task* prev;
        } Task;

        Task* current_task;
        Task* first_task;
        Task* last_task;
        extern void trace(const char*, ...);

        void add_task(Task* task) {
            if (last_task == 0) {
                current_task = task;
                last_task = task;
                first_task = task;
                task->next = task->prev = 0;
            } else {
                last_task->next = task;
                task->next = 0;
                task->prev = last_task;
                last_task = task;
            }

            task->state = 0;
            trace("add_task() : Added task : 0x%08X\n", (u32)task);
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.optimization = mwcc_versions::Optimization::O4;
    flags.ipa_file = true;
    let object = compile(
        source,
        "global-doubly-linked-append-trace.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_3_0A3,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the Revolution traced global queue append should compile");

    let expected = [
        0x80, 0x80, 0x00, 0x00, 0x2c, 0x04, 0x00, 0x00, 0x40, 0x82, 0x00, 0x20, 0x90, 0x60, 0x00,
        0x00, 0x38, 0x00, 0x00, 0x00, 0x90, 0x60, 0x00, 0x00, 0x90, 0x60, 0x00, 0x00, 0x90, 0x03,
        0x00, 0x3c, 0x90, 0x03, 0x00, 0x38, 0x48, 0x00, 0x00, 0x1c, 0x90, 0x64, 0x00, 0x38, 0x38,
        0x00, 0x00, 0x00, 0x90, 0x03, 0x00, 0x38, 0x80, 0x00, 0x00, 0x00, 0x90, 0x03, 0x00, 0x3c,
        0x90, 0x60, 0x00, 0x00, 0x38, 0x00, 0x00, 0x00, 0x3c, 0xa0, 0x00, 0x00, 0x90, 0x03, 0x00,
        0x00, 0x7c, 0x64, 0x1b, 0x78, 0x38, 0x65, 0x00, 0x00, 0x4c, 0xc6, 0x31, 0x82, 0x48, 0x00,
        0x00, 0x00,
    ];
    assert!(object
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}
