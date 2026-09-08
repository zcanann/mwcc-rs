// flags: -Cpp_exceptions off -pragma "cats off"
unsigned mode, position, enabled, scalar;
unsigned *cursor;
unsigned buffer[64];
void initialize(void) {
    mode = 0;
    position = 0;
    cursor = (unsigned *)&buffer;
    enabled = 1;
}
void leading_address(void) {
    cursor = buffer;
    mode = 7;
    position = 7;
    enabled = 3;
}
void small_address(void) {
    mode = 0;
    cursor = &scalar;
    position = 0;
    enabled = 1;
}
