// flags: -Cpp_exceptions off -pragma "cats off"
extern unsigned char done;
extern unsigned queue[2];
extern unsigned acquire(void);
extern void sleep_event(unsigned*);
extern void restore(unsigned);
static inline void wait_done(void) {
    unsigned token;
    token = acquire();
    while (!done) sleep_event(queue);
    restore(token);
}
void run_wait(void) { wait_done(); }
void guarded_wait(unsigned enabled) { if (enabled) wait_done(); }
