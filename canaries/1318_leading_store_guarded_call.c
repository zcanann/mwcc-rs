// builds: GC/1.1p1

typedef struct Buffer {
    int lock;
    int used;
    unsigned int length;
    unsigned int position;
    unsigned char data[2176];
} Buffer;

extern void clear_bytes(void* destination, int value, unsigned int size);

void reset_buffer(Buffer* buffer, unsigned char preserve)
{
    buffer->length = 0;
    buffer->position = 0;
    if (!preserve) {
        clear_bytes(buffer->data, 0, 2176);
    }
}
