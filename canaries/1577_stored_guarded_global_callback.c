// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned int u32;
typedef void (*Callback)(u32 interrupt, void* context);
extern unsigned char input_ready;
extern Callback notify_callback;
extern Callback interrupt_callback;
volatile u32 pi_registers[16] : 0xCC003000;

void notify_input(u32 interrupt, void* context) {
    input_ready = 1;
    if (notify_callback) notify_callback(0, context);
}

void acknowledge_interrupt(short interrupt, void* context) {
    pi_registers[0] = 0x1000;
    if (interrupt_callback) interrupt_callback(interrupt, context);
}
