// flags: -Cpp_exceptions off -pragma "cats off"
// Interrupt-token lifetimes extracted from the Melee transport wrappers.
typedef unsigned long u32;
typedef unsigned char u8;
typedef void (*Handler)(short, void*);
extern int enter_region(void);
extern void leave_region(int);
extern void mask_region(u32);
extern int read_words(u32, void*, u32);
volatile u32 registers[32] : 0xCC006800;
static Handler callback;
static u8* input_pointer;
static u8 input_flag;
static u32 mailbox;
static long remaining;

static void initialize_device(void) {
    mask_region(0x18000);
    registers[10] = 0;
}

void initialize_channel(int* result, int* handler) {
    int token;
    token = enter_region();
    input_pointer = &input_flag;
    *result = (int)input_pointer;
    callback = (Handler)handler;
    initialize_device();
    leave_region(token);
}

int read_channel(void* data, u32 size) {
    int token;
    u32 offset;
    token = enter_region();
    if (mailbox & 0x10000) {
        offset = 0x1000;
    } else {
        offset = 0;
    }
    read_words(offset + 0x1E000, data, (size + 3U) & 0xFFFFFFFC);
    remaining = 0;
    input_flag = 0;
    leave_region(token);
    return 0;
}
