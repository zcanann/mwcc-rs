// builds: GC/1.1 GC/2.6
// flags: -O4,p -inline auto -sdata 0 -sdata2 0 -Cpp_exceptions off

typedef struct MessageQueue {
    int words[8];
} MessageQueue;

extern MessageQueue message_queue;
extern int receive_message(MessageQueue* queue, void** message, int flag);

void* guarded_return_address_taken_local(int flag)
{
    void* message;
    if (receive_message(&message_queue, &message, flag) == 1) {
        return message;
    }
    return 0;
}
