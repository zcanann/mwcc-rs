// A fixed-count global-object-array walk. The element cursor and counter both
// survive a sequence of calls and must be colored r31/r30 as one loop region.
// builds: GC/1.1p1
// flags: -Cpp_exceptions off -sdata 0 -sdata2 0 -pool off -inline on,noauto -common off

typedef struct MessageBuffer {
    int mutex;
    int used;
    unsigned char payload[2184];
} MessageBuffer;

MessageBuffer message_buffers[3];

extern void initialize_mutex(MessageBuffer* buffer);
extern void acquire_mutex(MessageBuffer* buffer);
extern void mark_buffer_used(MessageBuffer* buffer, int used);
extern void release_mutex(MessageBuffer* buffer);

int initialize_message_buffers(void)
{
    int i;
    for (i = 0; i < 3; i++) {
        initialize_mutex(&message_buffers[i]);
        acquire_mutex(&message_buffers[i]);
        mark_buffer_used(&message_buffers[i], 0);
        release_mutex(&message_buffers[i]);
    }
    return 0;
}
