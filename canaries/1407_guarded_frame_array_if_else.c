// builds: GC/1.2.5n GC/1.3.2 GC/2.6

struct Device {
    int status;
    int value;
};

extern struct Device devices[];
extern int select_device(int channel, int slot, int frequency);
extern void clear_bytes(void* destination, int value, unsigned long length);
extern int transfer_device(
    int channel,
    const void* command,
    unsigned data,
    void* buffer,
    int length,
    int mode);

int guarded_frame_array_if_else(
    int channel,
    unsigned data,
    void* buffer,
    int length,
    int mode)
{
    struct Device* device;
    unsigned char command[5];
    device = &devices[channel];
    if (!select_device(channel, 0, 4))
        return -3;
    data &= 0xfffff000;
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
    transfer_device(channel, command, data, buffer, length, mode);
    return device->value;
}
