// flags: -Cpp_exceptions off -pragma "cats off"
void start(void);
void finish(void);
void poll(volatile unsigned int* status) {
    start();
    while (*status & 1) {}
    finish();
}
void wait_set(volatile unsigned int* status) {
    start();
    while (!(*status & 1)) {}
    finish();
}
