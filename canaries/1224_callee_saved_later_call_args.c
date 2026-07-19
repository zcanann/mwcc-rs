// builds: 1.3 1.3.2 2.0 2.0p1 2.6 2.7

typedef void (*Callback)(void);

void trace(int, const char*, ...);
void initialize(void*, Callback);

typedef struct Buffer {
    unsigned char* read;
    unsigned char* write;
    unsigned int size;
} Buffer;

void initialize_buffer(Buffer*, unsigned char*, unsigned int);

static Buffer buffer;
static unsigned char bytes[1280];

int initialize_after_trace(void* context, Callback callback)
{
    trace(1, "before");
    initialize(context, callback);
    trace(1, "after");
    initialize_buffer(&buffer, bytes, sizeof(bytes));
    return 0;
}
