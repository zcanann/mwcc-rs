// builds: GC/1.2.5n
// flags: -O4,p -inline auto -Cpp_exceptions off -pragma "cats off"

typedef unsigned int u32;
typedef int BOOL;
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

extern volatile BOOL canceling;
extern volatile u32 resume_from_here;
extern CommandBlock* executing;
extern CommandBlock dummy_block;
extern CommandCallback cancel_callback;
extern void ready(void);

BOOL guarded_deferred_callback_transaction(u32 resume)
{
    CommandBlock* finished;

    if (canceling) {
        resume_from_here = resume;
        canceling = 0;
        finished = executing;
        executing = &dummy_block;
        finished->state = 10;

        if (finished->callback) {
            finished->callback(-3, finished);
        }
        if (cancel_callback) {
            cancel_callback(0, finished);
        }

        ready();
        return 1;
    }
    return 0;
}
