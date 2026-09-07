// Outgoing stack slots must not become architectural register numbers.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
extern void sink(unsigned,unsigned,unsigned,unsigned,unsigned,unsigned,unsigned,unsigned,unsigned,unsigned,unsigned);
void forward(unsigned value) { sink(value,1,2,3,4,5,6,7,8,9,10); }
void computed(unsigned value) { sink(value,1,2,3,4,5,6,7,value+8,value+9,value+10); }
extern unsigned* values;
void loaded(unsigned value) { sink(value,1,2,3,4,5,6,7,values[0],values[1],values[2]); }
