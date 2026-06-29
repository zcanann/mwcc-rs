// On the PowerPC EABI target, `long` (and `long int`, `unsigned long`) is 32-bit, so it
// compiles exactly like `int`. `long long` is 64-bit (a register pair); its arithmetic
// (addc/adde, ...) is not modeled yet, so the parser DEFERS it rather than collapsing it to
// a 32-bit int and emitting 32-bit ops on a 64-bit value (which was a miscompile:
// `long long a + 1` became `addi r3,r3,1` instead of `addc r4..; adde r3..`).
long          long_add(long a)               { return a + 1; }       // addi, like int
long          long_mul(long a, long b)       { return a * b; }
unsigned long ulong_shift(unsigned long a)   { return a >> 2; }       // 32-bit logical shift
long          long_int_named(long int a)     { return a - 1; }
long double   longdouble_pass(long double a) { return a; }            // `long double` is a double

// DEFERRED (64-bit register-pair codegen not modeled):
//   long long      f(long long a)           { return a + 1; }
//   unsigned long long f(unsigned long long a) { return a; }
