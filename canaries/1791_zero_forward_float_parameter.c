// Zero forwarding must honor floating and narrowing parameter conversions.
// flags: -Cpp_exceptions off -pragma "cats off"
extern void sink(int,float,int,int,int,int,int,int,int,int);
void forward(int value) { sink(value,0,0,0,0,0,0,0,0,0); }
void macro_forward(int value) { (void)0; sink(value,0,0,0,0,0,0,0,0,0); }
extern void narrow_sink(unsigned char,int,int,int,int,int,int,int,int,int);
void narrow_forward(int value) { narrow_sink(value,0,0,0,0,0,0,0,0,0); }
void narrow_macro_forward(int value) { (void)0; narrow_sink(value,0,0,0,0,0,0,0,0,0); }
