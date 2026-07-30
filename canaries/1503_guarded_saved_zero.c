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

extern u32 retry_count;
extern CommandBlock* executing;
extern CommandBlock dummy_block;
extern void timeout(void);
extern void initialize_filesystem(void);
extern void ready(void);
extern void request_error(void);

void guarded_saved_zero(u32 interrupt)
{
    CommandBlock* finished;

    if (interrupt == 16) {
        timeout();
        return;
    }

    if (interrupt & 1) {
        retry_count = 0;
        initialize_filesystem();
        finished = executing;
        executing = &dummy_block;
        finished->state = 0;
        if (finished->callback) {
            finished->callback(0, finished);
        }
        ready();
        return;
    }

    request_error();
}
