// flags: -Cpp_exceptions off -pragma "cats off"
// Interrupt-token lifetimes extracted from the Melee transport wrappers.
typedef unsigned int u32;
typedef unsigned char u8;
typedef void (*Handler)(short, void*);
extern int acquire_state(void);
extern void release_state(int);
extern void mask_state(u32);
extern int receive_words(u32, void*, u32);
volatile u32 registers[32] : 0xCC00F000;
static Handler notification;
static u8* flag_location;
static u8 pending;
static u32 command_state;
static long pending_size;

static void initialize_device(void) {
    mask_state(0x28010);
    registers[7] = 0;
}

void configure_link(int* result, int* handler) {
    int token;
    token = acquire_state();
    flag_location = &pending;
    *result = (int)flag_location;
    notification = (Handler)handler;
    initialize_device();
    release_state(token);
}

int receive_link(void* data, u32 size) {
    int token;
    u32 offset;
    token = acquire_state();
    if (command_state & 0x40) {
        offset = 0x800;
    } else {
        offset = 0;
    }
    receive_words(offset + 0x34000, data, (size + 7U) & 0xFFFFFFF8);
    pending_size = 0;
    pending = 0;
    release_state(token);
    return -7;
}
