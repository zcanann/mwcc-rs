// An add-immediate whose result is a sub-expression (lands in the scratch): the
// operand still needs a non-scratch register, or addi d,r0,imm becomes li. The
// allocator now supplies it — ((a*b)+1) keeps the product in r3, then addi r3,r3,1.
// This is the trap that blocked the marioparty4 rand.c LCG (g*BIG + 0x3039).
int addimulscratch(int a, int b, int c){ return ((a * b) + 1) * c; }
