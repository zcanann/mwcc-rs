// flags: -sym on
/* A helper-produced destination remains live across a vector-copy transaction. */
typedef unsigned int u32;
void* map_address(u32 address);
void vector_begin(void);
void vector_end(void);
void copy_range(void* destination, void* source, u32 size);
void flush_data(void* destination, u32 size);
void invalidate_code(void* destination, u32 size);
void __sync(void);
static asm void vector_stub(void) {
entry vector_begin
    nofralloc
    li r3, 0
    blr
entry vector_end
    nop
}
void install_mapped_vector(void) {
    void* destination = map_address(0x900);
    copy_range(destination, vector_begin, (u32)&vector_end - (u32)&vector_begin);
    flush_data(destination, 0x80);
    __sync();
    invalidate_code(destination, 0x80);
}
