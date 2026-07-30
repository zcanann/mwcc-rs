// builds: GC/1.2.5n
// flags: -O4,p -inline auto -Cpp_exceptions off -pragma "cats off"

typedef unsigned int u32;
typedef struct CommandBlock CommandBlock;
typedef void (*CommandCallback)(int, CommandBlock*);

struct CommandBlock {
    u32 command;
    u32 offset;
    u32 length;
    int state;
    u32 pad[6];
    CommandCallback callback;
};

extern CommandBlock* executing;
extern CommandBlock dummy_block;
extern volatile int fatal_error;
extern volatile int canceling;
extern CommandCallback cancel_callback;
extern void timeout(void);
extern void report_fatal(void);
extern void ready(void);

void post_call_deferred_callback_transaction(u32 interrupt)
{
    CommandBlock* finished;

    executing->state = -1;
    if (interrupt == 16) {
        timeout();
        return;
    }

    report_fatal();
    fatal_error = 1;
    finished = executing;
    executing = &dummy_block;

    if (finished->callback) {
        finished->callback(-1, finished);
    }
    if (canceling) {
        canceling = 0;
        if (cancel_callback) {
            cancel_callback(0, finished);
        }
    }
    ready();
}
