// builds: GC/1.2.5 GC/1.3 GC/1.3.2 GC/2.0 GC/2.0p1 GC/2.6 GC/2.7

typedef struct Thread Thread;
typedef struct Mutex Mutex;

typedef struct ThreadQueue {
    Thread* head;
    Thread* tail;
} ThreadQueue;

typedef struct MutexLink {
    Mutex* next;
    Mutex* previous;
} MutexLink;

typedef struct MutexQueue {
    Mutex* head;
    Mutex* tail;
} MutexQueue;

struct Thread {
    int priority;
    int base_priority;
    MutexQueue mutexes;
};

struct Mutex {
    ThreadQueue blocked;
    MutexLink link;
};

int effective_priority(Thread* thread)
{
    int priority;
    Mutex* mutex;
    Thread* blocked;

    priority = thread->base_priority;
    for (mutex = thread->mutexes.head; mutex; mutex = mutex->link.next) {
        blocked = mutex->blocked.head;
        if (blocked && blocked->priority < priority) {
            priority = blocked->priority;
        }
    }
    return priority;
}
