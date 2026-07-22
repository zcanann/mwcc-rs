// builds: GC/1.2.5n
// flags: -O4,s -inline off -Cpp_exceptions off -pragma "cats off" -func_align 32 -use_lmw_stmw on

typedef unsigned char u8;
typedef unsigned int u32;

struct Task {
    u8 bytes[0x50];
};

struct Record {
    u8 prefix[0x14];
    u32 latency;
    u8 middle[0x10];
    u32 format_step;
    u8 gap[4];
    Task task;
    void* work_area;
    u8 suffix[0x84];
};

extern Record records[2];
extern int produce_length();
extern void first_use(int channel, Record* record, u8* buffer, int length);
extern void second_use(int channel, Record* record, u32 answer, int length);
extern void consume_shift(u32 state, u32 shift);

static void dispatch(void* context)
{
    Task* task = (Task*)context;
    u8 buffer[64];
    u32 answer;
    int channel;
    Record* record;

    for (channel = 0; channel < 2; ++channel) {
        record = &records[channel];
        if (&record->task == task) {
            break;
        }
    }

    u8* input = (u8*)record->work_area + 16;
    input = (u8*)(((u32)input + 31) & ~31);
    answer = *(u32*)(input + 32);
    int length;
    length = produce_length();
    first_use(channel, record, buffer, length);
    second_use(channel, record, answer, length);

    u32 shift;
    shift = (length + 4 + record->latency) * 8 + 1;
    consume_shift(record->format_step, shift);
}
