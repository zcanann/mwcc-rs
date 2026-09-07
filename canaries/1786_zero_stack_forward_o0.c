// GX-style zero forwarding with and without a macro no-op.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
extern void sink(int,int,int,int,int,int,int,int,int,int);
void forward(int value) { sink(value,0,0,0,0,0,0,0,0,0); }
void macro_forward(int value) { (void)0; sink(value,0,0,0,0,0,0,0,0,0); }
