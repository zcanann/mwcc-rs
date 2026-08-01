// builds: GC/1.1 GC/2.6
// flags: -O4,p -inline auto -sdata 0 -sdata2 0 -Cpp_exceptions off

typedef struct MessageQueue {
    int words[8];
} MessageQueue;

extern MessageQueue message_queue;
extern int send_message(MessageQueue* queue, void* message, int flag);

void global_queue_pointer_send(void* message)
{
    send_message(&message_queue, message, 0);
}
