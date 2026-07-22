// builds: GC/1.2.5n
// flags: -Cpp_exceptions off

typedef struct Box {
    void* value;
} Box;

static inline void* box_value(Box* box)
{
    return box->value;
}

extern void consume_pointer(void*);

void legacy_inline_frame_residue(Box* box)
{
    void* value = box_value(box);
    consume_pointer(value);
    consume_pointer(box);
}
