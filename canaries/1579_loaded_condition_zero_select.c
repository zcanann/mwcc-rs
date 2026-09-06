// flags: -Cpp_exceptions off -pragma "cats off"
extern unsigned char send_count;
extern unsigned int status;
void submit(unsigned int value);

unsigned int select_mailbox_bit(void) {
    return (send_count & 1) ? 0x1000 : 0;
}

void submit_mailbox_bit(void) {
    unsigned int value;
    value = (send_count & 1) ? 0x1000 : 0;
    submit(value | 0x1C000);
}

unsigned int select_clear_status(void) {
    return (status & 2) ? 0 : 0x80;
}
