/* Address alignment uses the symbol address, never element zero. */
extern char stack_end[];
extern unsigned words[];
extern char *cursor;
extern unsigned address_word;
char small[4];
char large[64];
char *align_symbol(void) { return (char*)(((unsigned)stack_end + 31) & ~31U); }
char *align_words(void) { return (char*)(((unsigned)words + 31) & ~31U); }
char *align_small(void) { return (char*)(((unsigned)small + 31) & ~31U); }
char *align_large(void) { return (char*)(((unsigned)large + 31) & ~31U); }
char *align_cursor(void) { return (char*)(((unsigned)cursor + 31) & ~31U); }
char *align_word(void) { return (char*)((address_word + 31) & ~31U); }
char *align_displaced(void) { return (char*)(((unsigned)stack_end + 95) & ~31U); }
char *align_shadow(char *stack_end) { return (char*)(((unsigned)stack_end + 31) & ~31U); }
extern unsigned *debug_flag;
char *align_guarded(void) {
    if (debug_flag && *debug_flag < 2)
        return (char*)(((unsigned)stack_end + 31) & ~31U);
    return 0;
}
