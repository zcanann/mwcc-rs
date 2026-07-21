// builds: GC/1.1p1

typedef struct Buffer {
    int lock;
    int used;
    unsigned int length;
    unsigned int position;
    unsigned char data[2176];
} Buffer;

extern void* copy_bytes(void* destination, const void* source, unsigned int size);

int append_buffer(Buffer* buffer, const void* data, unsigned int length)
{
    int error = 0;
    unsigned int available;

    if (length == 0) {
        return 0;
    }
    available = 2176 - buffer->position;
    if (available < length) {
        error = 769;
        length = available;
    }
    if (length == 1) {
        buffer->data[buffer->position] = ((unsigned char*)data)[0];
    } else {
        copy_bytes(buffer->data + buffer->position, data, length);
    }
    buffer->position += length;
    buffer->length = buffer->position;
    return error;
}
