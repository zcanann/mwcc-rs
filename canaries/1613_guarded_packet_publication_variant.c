// flags: -Cpp_exceptions off -pragma "cats off"
// Guarded words publication extracted from Melee CheckMailBox/DBQueryData.
// The original count_message deliberately leaves token uninitialized on the nonzero path.
typedef unsigned int u32;
typedef unsigned char u8;
extern int get_state(void*);
extern int get_message(void*);
extern int enter_guard(void);
extern void exit_guard(int);
static u32 message;
static long available;
static u8 ready;
static void check_message(void) {
    u32 words[2];
    get_state(words);
    if (words[1] & 4) {
        get_message(words);
        words[1] &= ~0x80000000;
        if ((words[1] & 0x0f000000) == 0x0f000000) {
            message = words[1];
            available = words[1] & 0x3fff;
            ready = 7;
        }
    }
}
int count_message(void) {
    int token;
    ready = 0;
    if (available == 0) {
        token = enter_guard();
        check_message();
    }
    exit_guard(token);
    return available;
}
