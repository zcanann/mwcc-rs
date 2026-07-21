typedef unsigned int u32;

void vector_begin(void);
void vector_end(void);
void copy_range(void* destination, void* source, u32 size);
void flush_data(void* destination, u32 size);
void invalidate_code(void* destination, u32 size);

void install_vector(void) {
    void* destination = (void*)0x80000C00;
    copy_range(destination, vector_begin, (u32)&vector_end - (u32)&vector_begin);
    flush_data(destination, 0x100);
    __sync();
    invalidate_code(destination, 0x100);
}
