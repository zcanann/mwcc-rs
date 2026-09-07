// A computed floating local forwarded to a prototyped call.
// flags: -Cpp_exceptions off -pragma "cats off" -fp_contract off
extern float accept(float);
extern double accept_double(double);
float statement(float x) { float r; r=x*3.0f/180.0f; return accept(r); }
float initialized(float x) { float r=x*3.0f/180.0f; return accept(r); }
float switched(float x, int c) { float r, y; r=x*3.0f/180.0f; y=accept(r); switch(c) {case 1: return -y; case 2: return y*y; default:return y;} }
double statement_double(double x) { double r; r=x*3.0/180.0; return accept_double(r); }
