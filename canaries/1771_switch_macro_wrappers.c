// Switch macro and GX context execution coverage.
// flags: -Cpp_exceptions off -pragma "cats off"
extern unsigned state;
#define ADD(x) do { state += (x); } while (0)
void select(int n) {
 switch (n) {
 case 0: ADD(3); break;
 case 1: ADD(5);
 case 2: ADD(7); break;
 default: ADD(11); break;
 }
 ADD(13);
}
void nested(int n, int m) {
 switch(n) {
 case 0: do { switch(m) { case 2: ADD(17); break; default: ADD(19); break; } } while(0); break;
 default: ADD(23); break;
 }
}
