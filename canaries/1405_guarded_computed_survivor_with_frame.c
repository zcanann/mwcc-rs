// builds: GC/1.2.5n GC/1.3.2 GC/2.6

struct Device {
    int status;
    int value;
};

extern struct Device devices[];
extern int select_device(int channel, int slot, int frequency);
extern void clear_bytes(void* destination, int value, unsigned long length);
extern void consume_bytes(const void* bytes);

int guarded_device_buffer(int channel)
{
    struct Device* device;
    unsigned char command[5];
    device = &devices[channel];
    if (!select_device(channel, 0, 4))
        return -3;
    clear_bytes(command, 0, sizeof(command));
    command[0] = 82;
    consume_bytes(command);
    return device->value;
}
