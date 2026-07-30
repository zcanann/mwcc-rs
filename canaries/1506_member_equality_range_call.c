// builds: GC/1.2.5n
// flags: -O4,p -inline auto -Cpp_exceptions off -pragma "cats off"

typedef unsigned int u32;

typedef struct CommandBlock {
    u32 pad0;
    u32 pad1;
    u32 command;
    u32 pad3;
    u32 pad4;
    u32 length;
    void* address;
} CommandBlock;

extern int invalidation_enabled;
extern void invalidate_range(void* address, u32 length);

void member_equality_range_call(CommandBlock* block)
{
    if (invalidation_enabled != 0
        && (block->command == 1
            || block->command == 4
            || block->command == 5
            || block->command == 14)) {
        invalidate_range(block->address, block->length);
    }
}
