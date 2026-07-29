// Build 163 keeps the condition after its complete linkage-first prologue,
// preserves the endangered size argument with `mr`, and restores LR before
// restoring the stack pointer in a saved-GPR epilogue.
// builds: GC/1.2.5 GC/1.2.5n
// flags: -Cpp_exceptions off

extern void* memset(void* destination, int value, unsigned long size);
extern void* memcpy(void* destination, const void* source, unsigned long size);
extern void flush_cache(void* destination, unsigned long size);

asm void startup(void)
{
    nofralloc
    blr
}

void initialize_bss(void* destination, unsigned long size)
{
    if (size) {
        memset(destination, 0, size);
    }
}

void copy_section(void* destination, const void* source, unsigned long size)
{
    if (size && destination != source) {
        memcpy(destination, source, size);
        flush_cache(destination, size);
    }
}
