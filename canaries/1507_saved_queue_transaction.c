// builds: GC/1.2.5n
// flags: -O4,p -inline auto -Cpp_exceptions off -pragma "cats off"

typedef unsigned int u32;

typedef struct CommandBlock {
    u32 pad0;
    u32 pad1;
    u32 command;
    int state;
    u32 pad4;
    u32 length;
    void* address;
} CommandBlock;

extern int invalidation_enabled;
extern CommandBlock* current_command;
extern int pause_requested;
extern void invalidate_range(void* address, u32 length);
extern int disable_interrupts(void);
extern void restore_interrupts(int level);
extern int push_waiting_command(int priority, CommandBlock* block);
extern void ready(void);

int saved_queue_transaction(int priority, CommandBlock* block)
{
    int level;
    int result;

    if (invalidation_enabled != 0
        && (block->command == 1
            || block->command == 4
            || block->command == 5
            || block->command == 14)) {
        invalidate_range(block->address, block->length);
    }

    level = disable_interrupts();
    block->state = 2;
    result = push_waiting_command(priority, block);
    if (current_command == 0 && pause_requested == 0) {
        ready();
    }
    restore_interrupts(level);
    return result;
}
