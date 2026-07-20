// A locked global queue post: reject a full queue, otherwise choose the next
// slot, copy an event, assign a monotonic ID, update the count, and unlock.
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

int post_event(const Event* event) {
    int status = 0;
    int next_event_id;

    acquire_mutex(&queue);
    if (queue.count == 2) {
        status = 0x100;
    } else {
        next_event_id = (queue.next + queue.count) % 2;
        copy_event(&queue.events[next_event_id], event);
        queue.events[next_event_id].id = queue.id;
        queue.id++;
        if (queue.id < 0x100) {
            queue.id = 0x100;
        }
        queue.count++;
    }
    release_mutex(&queue);
    return status;
}
