// A callback queue accumulates failed indirect results, then folds one direct synchronization
// result into the same saved boolean before returning its inverse.
// builds: GC/1.2.5
typedef int (*QueueCallback)(int);
typedef struct CallbackNode {
    QueueCallback callback;
    unsigned priority;
    struct CallbackNode* next;
    struct CallbackNode* previous;
} CallbackNode;
typedef struct CallbackQueue { CallbackNode* head; CallbackNode* tail; } CallbackQueue;
static CallbackQueue queue;
extern int synchronize(void);

int queue_callback_fold(int final) {
    CallbackNode* iterator;
    int failed = 0;
    for (iterator = queue.head; iterator != 0; iterator = iterator->next) {
        failed |= !iterator->callback(final);
    }
    failed |= !synchronize();
    if (failed) {
        return 0;
    }
    return 1;
}
