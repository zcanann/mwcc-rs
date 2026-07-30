// builds: GC/1.2.5n
// flags: -O4,p -inline auto -Cpp_exceptions off -pragma "cats off" -fp_contract off -char signed

typedef unsigned int u32;

enum StreamState {
    STREAM_STATE_FATAL = -1,
    STREAM_STATE_END = 0,
    STREAM_STATE_CANCELED = 10
};

typedef struct CommandBlock CommandBlock;
typedef void (*CommandCallback)(int result, CommandBlock* block);

struct CommandBlock {
    u32 pad0;
    u32 pad1;
    u32 command;
    int state;
    u32 pad4;
    u32 pad5;
    u32 pad6;
    u32 pad7;
    u32 transferred_size;
    u32 pad8;
    CommandCallback callback;
    u32 pad11;
};

extern int thread_queue;
extern int issue_command(int priority, CommandBlock* block);
extern int disable_interrupts(void);
extern void restore_interrupts(int level);
extern void sleep_thread(int* queue);

static void stream_callback(int result, CommandBlock* block)
{
    block->transferred_size = (u32)result;
}

static inline int cancel_stream_async(CommandBlock* block, CommandCallback callback)
{
    int idle;

    block->command = 7;
    block->callback = callback;
    idle = issue_command(1, block);
    return idle;
}

int cancel_stream(CommandBlock* block)
{
    int result;
    int state;
    int enabled;
    int return_value;

    result = cancel_stream_async(block, stream_callback);
    if (result == 0) {
        return -1;
    }
    enabled = disable_interrupts();
    while (1) {
        state = block->state;
        if ((u32)(state - STREAM_STATE_FATAL) <= 1
            || state == STREAM_STATE_CANCELED) {
            return_value = block->transferred_size;
            break;
        }
        sleep_thread(&thread_queue);
    }
    restore_interrupts(enabled);
    return return_value;
}
