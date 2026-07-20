// Dolphin's reset callback registry inserts nodes by priority into an intrusive doubly linked
// queue. Build 145 folds the tail address through lwzu and schedules the predecessor repair.
// builds: GC/1.2.5
typedef struct InsertNode {
    void* callback;
    unsigned priority;
    struct InsertNode* next;
    struct InsertNode* previous;
} InsertNode;

typedef struct InsertQueue {
    InsertNode* head;
    InsertNode* tail;
} InsertQueue;

static InsertQueue queue;

void sorted_intrusive_insert(InsertNode* item) {
    InsertNode* temporary;
    InsertNode* iterator;
    for (iterator = queue.head;
         iterator && iterator->priority <= item->priority;
         iterator = iterator->next) {
    }
    if (iterator == 0) {
        temporary = queue.tail;
        if (temporary == 0) {
            queue.head = item;
        } else {
            temporary->next = item;
        }
        item->previous = temporary;
        item->next = 0;
        queue.tail = item;
        return;
    }
    item->next = iterator;
    temporary = iterator->previous;
    iterator->previous = item;
    item->previous = temporary;
    if (temporary == 0) {
        queue.head = item;
        return;
    }
    temporary->next = item;
}
