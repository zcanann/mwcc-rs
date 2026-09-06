// flags: -Cpp_exceptions off -pragma "cats off" -inline off
int select_channel(unsigned int channel);
int transfer(void* data, int size, unsigned int write);
int synchronize(void);
int deselect(void);

int read_mailbox(void* data) {
    int error = 0;
    unsigned int command;
    if (!select_channel(4)) return 0;
    command = 0x60000000;
    error |= !transfer(&command, 2, 1);
    error |= !synchronize();
    error |= !transfer(data, 4, 0);
    error |= !synchronize();
    error |= !deselect();
    return !error;
}

int retain_initial_error(int initial) {
    int error = initial;
    if (!select_channel(4)) return 0;
    error |= !synchronize();
    error |= !deselect();
    return !error;
}
