// flags: -Cpp_exceptions off -pragma "cats off"
unsigned short buffers[2][384];
unsigned position;
unsigned short *cursor;
unsigned short *address(unsigned row, unsigned column) { return &buffers[row][column]; }
unsigned short *row_address(unsigned row) { return buffers[row]; }
unsigned read(unsigned row, unsigned column) { return buffers[row][column]; }
void write(unsigned row, unsigned column, unsigned value) { buffers[row][column] = value; }
unsigned exchange(void) {
    unsigned address = (unsigned)&buffers[position][0];
    position += 1;
    position &= 1;
    cursor = &buffers[position][0];
    return address;
}
unsigned row_size(void) { return sizeof(buffers[0]); }
