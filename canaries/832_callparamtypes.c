// Call arguments are placed in the register the PARAMETER's type requires (a float
// parameter takes f1.., an integer takes r3..), now that prototypes carry parameter types.
// When the argument's float-ness matches the parameter's, the placement is byte-exact; a
// mismatch (int arg to a float parameter, or vice versa) needs an int<->float conversion at
// the call site, which is not modeled yet, so it DEFERS (rather than the previous miscompile
// of passing an int in r3 to a parameter read from f1). The DEFER cases are commented below.
void take_f(float);
void take_i(int);
void take_if(int, float);
void take_ff(float, float);
int  square_i(int);
int  square_f(float);

void call_float(float a)        { take_f(a); }            // f1 <- a
void call_int(int a)            { take_i(a); }            // r3 <- a
void call_mixed(int a, float b) { take_if(a, b); }        // r3 <- a, f1 <- b
void call_two_float(float a, float b) { take_ff(a, b); }  // f1 <- a, f2 <- b
int  call_ret_int(int a)        { return square_i(a); }   // r3 <- a, result in r3
int  call_ret_int_float(float a){ return square_f(a); }   // f1 <- a, result in r3

// DEFERRED (need a call-site int<->float conversion, not modeled):
//   void f(int a)   { take_f(a); }   // int -> float parameter
//   void f(float a) { take_i(a); }   // float -> int parameter
