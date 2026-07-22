// builds: GC/1.2.5n GC/1.3.2 GC/2.6
// flags: -O4,s -inline off -Cpp_exceptions off -pragma "cats off" -func_align 32

typedef int BOOL;
typedef unsigned char u8;
typedef unsigned int u32;

struct WorkArea {
    u8 header_padding[512];
    u8 buffer[32];
};

struct CardControl {
    u8 prefix[20];
    int latency;
    u8 middle[104];
    struct WorkArea* work_area;
};

extern struct CardControl card_blocks[];
extern void panic(const char* file, int line, const char* message, ...);
extern int select_device(int channel, int slot, int frequency);
extern void clear_bytes(void* destination, int value, unsigned long length);
extern int transfer_device(int channel, const void* data, int length, int mode);
extern int deselect_device(int channel);

static int card_style_read_array_unlock(
    int channel,
    u32 data,
    void* read_buffer,
    int length,
    BOOL mode)
{
    (void)(((0 <= channel && channel < 2)
                || (panic("card_unlock.c", 216, "channel assertion"), 0)));
    struct CardControl* card = &card_blocks[channel];

    if (!select_device(channel, 0, 4))
        return -3;

    data &= 0xfffff000;
    u8 command[5];
    clear_bytes(command, 0, sizeof(command));
    command[0] = 82;
    if (mode == 0) {
        command[1] = (data >> 29) & 3;
        command[2] = (data >> 21) & 255;
        command[3] = (data >> 19) & 3;
        command[4] = (data >> 12) & 127;
    } else {
        command[1] = (data >> 24) & 255;
        command[2] = (data >> 16) & 255;
    }

    BOOL error = 0;
    error |= !transfer_device(channel, command, sizeof(command), 1);
    error |= !transfer_device(channel, card->work_area->buffer, card->latency, 1);
    error |= !transfer_device(channel, read_buffer, length, 0);
    error |= !deselect_device(channel);
    return error ? -3 : 0;
}
