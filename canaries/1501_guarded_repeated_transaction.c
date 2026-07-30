// builds: GC/1.2.5n
// flags: -O4,p -inline auto -Cpp_exceptions off -pragma "cats off"

typedef unsigned int u32;
typedef void (*Callback)(u32);

extern void store_error(u32);
extern void reset_device(void);
extern void error_complete(u32);
extern void retry(void);
extern void request_error(Callback);
extern void observe(u32);

static void report_timeout(void)
{
    store_error(0x01234568);
    reset_device();
    error_complete(0);
}

void guarded_repeated_transaction(u32 interrupt)
{
    if (interrupt == 0x10) {
        report_timeout();
        return;
    }
    if (interrupt & 1) {
        retry();
        return;
    }
    request_error(error_complete);
}

void retain_repeated_transaction(u32 value)
{
    observe(value);
    report_timeout();
    observe(value);
}
