// flags: -sym on -Cpp_exceptions off
/* Entry labels retain word alignment even when functions are aligned to 16. */
void entry_begin(void);
void entry_tail(void);
static asm void labeled_body(void) {
entry entry_begin
    nofralloc
    li r3, 1
entry entry_tail
    blr
}
