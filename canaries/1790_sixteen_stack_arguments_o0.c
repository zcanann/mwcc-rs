// Outgoing stack slots must not become architectural register numbers.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
extern void sink(unsigned,unsigned,unsigned,unsigned,unsigned,unsigned,unsigned,unsigned,unsigned,unsigned,unsigned,unsigned,unsigned,unsigned,unsigned,unsigned);
void forward(unsigned value) { sink(value,1,2,3,4,5,6,7,0x12345678,0x87654321,0xdeadbeef,0x80000001,12,13,14,15); }
