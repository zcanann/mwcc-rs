// builds: GC/1.1p1

typedef struct Buffer {
    int lock;
    int used;
    unsigned int length;
    unsigned int position;
    unsigned char data[2176];
} Buffer;

static inline int append_one(Buffer* buffer, const unsigned char data)
{
    if (buffer->position >= 2176) {
        return 769;
    }
    buffer->data[buffer->position++] = data;
    buffer->length++;
    return 0;
}

int append_many(Buffer* buffer, const unsigned char* data, int count)
{
    int error;
    int i;
    for (i = 0, error = 0; error == 0 && i < count; i++) {
        error = append_one(buffer, data[i]);
    }
    return error;
}
