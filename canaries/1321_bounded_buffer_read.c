// builds: GC/1.1p1

typedef struct Buffer {
    int lock;
    int used;
    unsigned int length;
    unsigned int position;
    unsigned char data[2176];
} Buffer;

extern void* copy_bytes(void* destination, const void* source, unsigned int size);

int read_buffer(Buffer* buffer, void* data, unsigned int length)
{
    int error = 0;
    unsigned int available;

    if (length == 0) {
        return 0;
    }
    available = buffer->length - buffer->position;
    if (length > available) {
        error = 770;
        length = available;
    }
    copy_bytes(data, buffer->data + buffer->position, length);
    buffer->position += length;
    return error;
}
