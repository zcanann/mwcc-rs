struct Device {
    int status;
    int value;
};

extern struct Device devices[];
extern int select_device(int channel, int slot, int frequency);

int guarded_device_value(int channel)
{
    struct Device* device;
    device = &devices[channel];
    if (!select_device(channel, 0, 4))
        return -3;
    return device->value;
}
