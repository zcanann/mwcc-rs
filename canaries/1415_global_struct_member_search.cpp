// builds: GC/1.2.5n GC/1.3.2 GC/2.6
// flags: -O4,s -inline off -Cpp_exceptions off -pragma "cats off" -func_align 32 -use_lmw_stmw on

typedef unsigned char u8;

struct Task {
    u8 bytes[0x50];
};

struct Card {
    u8 prefix[0x30];
    Task task;
    u8 suffix[0x90];
};

extern Card blocks[2];
extern void consume(int channel, Card* card);

static void find_task(Task* task)
{
    int channel;
    Card* card;

    for (channel = 0; channel < 2; ++channel) {
        card = &blocks[channel];
        if (&card->task == task) {
            break;
        }
    }

    consume(channel, card);
}
