// A global aggregate whose address stays live across initialization calls and
// a constant member-store batch. The legacy compiler parks the base in r31.
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

extern int initialize_mutex(void* mutex);
extern int acquire_mutex(void* mutex);
extern int release_mutex(void* mutex);

Queue queue;

int initialize_queue(void) {
    initialize_mutex(&queue);
    acquire_mutex(&queue);
    queue.count = 0;
    queue.next = 0;
    queue.id = 0x100;
    release_mutex(&queue);
    return 0;
}
