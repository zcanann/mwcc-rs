// builds: GC/1.2.5n
// flags: -O4,p -inline auto -Cpp_exceptions off -pragma "cats off"

typedef unsigned int u32;
typedef void (*Callback)(u32);

volatile u32 command_registers[] : 0xCC006000;
extern void store_error(u32);
extern void stop_motor(Callback);
extern void reset_device(void);
extern void error_complete(u32);

static void report_error(u32 error)
{
    store_error(error);
    stop_motor(error_complete);
}

static void report_timeout(void)
{
    store_error(0x01234568);
    reset_device();
    error_complete(0);
}

void nested_terminal_inline_argument(u32 interrupt)
{
    if (interrupt == 0x10) {
        report_timeout();
    } else {
        if (interrupt & 2) {
            report_error(0x01234567);
            return;
        }
        report_error(command_registers[8]);
    }
}
