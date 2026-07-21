// A same-valued terminal global-store run materializes the stored constant
// once and schedules the independent return value before the final store.
// builds: GC/1.1 GC/1.3.2 GC/2.6

extern int first;
extern int second;
extern void touch(void);

int store_run(void) {
    touch();
    first = 1;
    second = 1;
    return 1;
}
