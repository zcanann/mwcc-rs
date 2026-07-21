// A queue post may inline a verified fixed-size copy wrapper and reuse the
// input's callee-saved register for the selected entry's scaled offset.
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

int submit_entry(const Entry* entry)
{
    int result = 0;
    int selected;

    enter_queue(&work_queue);
    if (work_queue.count == 2) {
        result = 0x100;
    } else {
        selected = (work_queue.next + work_queue.count) % 2;
        copy_entry(&work_queue.entries[selected], entry);
        work_queue.entries[selected].identifier = work_queue.identifier;
        work_queue.identifier++;
        if (work_queue.identifier < 0x100) {
            work_queue.identifier = 0x100;
        }
        work_queue.count++;
    }
    leave_queue(&work_queue);
    return result;
}
