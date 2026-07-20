// A locked global queue pop: conditionally copy one indexed entry, update the
// count/head with wraparound, then unlock and return whether an entry existed.
// builds: GC/1.1p1
// flags: -pool off -str readonly -enum min -sdatathreshold 0
typedef struct Event {
    int kind;
    unsigned id;
    int message;
} Event;

typedef struct Queue {
    int mutex;
    int count;
    int next;
    Event events[2];
    unsigned id;
} Queue;

extern int acquire_mutex(void* mutex);
extern int release_mutex(void* mutex);
extern void copy_event(Event* output, const Event* input);

Queue queue;

int get_next_event(Event* output) {
    int status = 0;
    acquire_mutex(&queue);
    if (0 < queue.count) {
        copy_event(output, &queue.events[queue.next]);
        queue.count--;
        queue.next++;
        if (queue.next == 2) {
            queue.next = 0;
        }
        status = 1;
    }
    release_mutex(&queue);
    return status;
}
