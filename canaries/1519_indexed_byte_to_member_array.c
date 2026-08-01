// builds: GC/1.1p1 GC/2.6
// flags: -O4,p -inline on,noauto -sdata 0 -sdata2 0 -pool off

typedef unsigned char u8;
typedef unsigned int u32;

typedef struct Buffer {
    u32 length;
    u32 position;
    u8 data[32];
} Buffer;

void indexed_byte_to_member_array(Buffer* buffer, const void* source)
{
    buffer->data[buffer->position] = ((const u8*)source)[0];
    buffer->length = buffer->position;
}
