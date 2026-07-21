// A queue pop may inline a verified fixed-size copy wrapper; the wrapper's
// third size argument then participates in the caller's register schedule.
// builds: GC/1.3
// flags: -sym off -Cpp_exceptions off -pool off -str readonly -enum min -sdatathreshold 0 -inline auto,deferred

typedef struct Entry {
    int kind;
    unsigned identifier;
    int message;
} Entry;

typedef struct WorkQueue {
    int mutex;
    int count;
    int next;
    Entry entries[2];
    unsigned identifier;
} WorkQueue;

extern int enter_queue(void* mutex);
extern int leave_queue(void* mutex);
extern void copy_bytes(void* destination, const void* source, unsigned byte_count);

WorkQueue work_queue;

void copy_entry(Entry* destination, const Entry* source)
{
    copy_bytes(destination, source, sizeof(Entry));
}

int take_entry(Entry* output)
{
    int found = 0;
    enter_queue(&work_queue);
    if (0 < work_queue.count) {
        copy_entry(output, &work_queue.entries[work_queue.next]);
        work_queue.count--;
        work_queue.next++;
        if (work_queue.next == 2) {
            work_queue.next = 0;
        }
        found = 1;
    }
    leave_queue(&work_queue);
    return found;
}
