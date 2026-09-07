// flags: -Cpp_exceptions off -pragma "cats off"
// Guarded packet publication extracted from Melee CheckMailBox/DBQueryData.
// The original query deliberately leaves enabled uninitialized on the nonzero path.
typedef unsigned long u32;
typedef unsigned char u8;
extern int read_status(void*);
extern int read_mailbox(void*);
extern int acquire(void);
extern void release(int);
static u32 mailbox;
static long remaining;
static u8 pending;
static void poll_mailbox(void) {
    u32 packet[2];
    read_status(packet);
    if (packet[0] & 1) {
        read_mailbox(packet);
        packet[0] &= ~0xe0000000;
        if ((packet[0] & 0x1f000000) == 0x1f000000) {
            mailbox = packet[0];
            remaining = packet[0] & 0x7fff;
            pending = 1;
        }
    }
}
int query(void) {
    int enabled;
    pending = 0;
    if (remaining == 0) {
        enabled = acquire();
        poll_mailbox();
    }
    release(enabled);
    return remaining;
}
