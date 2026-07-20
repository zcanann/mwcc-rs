// A call result stored to an absolute global can feed a trailing guarded call
// block through the updated address base. Build 159 folds the low relocation
// into `stwu`, reloads through that base, and preserves it across the branch.
// builds: GC/1.1 GC/1.1p1 GC/1.2.5 GC/1.2.5n GC/1.3 GC/1.3.2 GC/2.0 GC/2.0p1 GC/2.5 GC/2.6 GC/2.7 GC/3.0a3 GC/3.0a3p1
// flags: -Cpp_exceptions off -O4,p -inline on,noauto -fp_contract off -sdata 0 -sdata2 0 -pool off
static int status;

extern int initialize(void);
extern void welcome(void);
extern void run(void);
extern int terminate(void);

int global_call_store_guard_tail(void) {
    status = initialize();
    if (status == 0) {
        welcome();
        run();
    }
    return status = terminate();
}
