// A call result used as the operand of an immediate op, stored — `gi = foo(a) & 0xff;`.
// The result lives in r3; mwcc reads it in place (`bl foo; rlwinm r0,r3,0,24,31; stw r0`)
// rather than bouncing it through the scratch. Placing the call operand in a fresh virtual
// (which the allocator colors to r3, the resulting mr r3,r3 coalesced away) reproduces this
// for every immediate op, the same way the add-immediate path already did. Non-call
// operands are unaffected — they keep their scratch/destination placement.
int produce(int);
int gi;
void mask_result(int a)   { gi = produce(a) & 0xff; }   // rlwinm r0,r3,0,24,31
void shift_pow2(int a)    { gi = produce(a) * 2; }      // slwi  r0,r3,1
void shift_left(int a)    { gi = produce(a) << 3; }     // slwi  r0,r3,3
void or_const(int a)      { gi = produce(a) | 16; }     // ori   r0,r3,16
void mul_const(int a)     { gi = produce(a) * 5; }      // mulli r0,r3,5
void add_const(int a)     { gi = produce(a) + 1; }      // addi  r0,r3,1
