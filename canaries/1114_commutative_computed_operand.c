// A COMMUTATIVE op (`*`, `&`, `|`, `^`) whose LEFT operand is a SINGLE-REGISTER-computed value —
// one register leaf via addi/subfic/neg (`a-1`, `2-a`, `-a`) — and whose RIGHT is a bare register
// leaf: mwcc keeps the computed operand in source order as rA (`addi r0,r3,-1; mullw r3,r0,r4`),
// NOT anchoring the leaf. place_general_operands now orders it computed-first (like a constant
// shift), so these compile byte-exact instead of deferring. A two-register computed operand
// (`(a+b)*c`), a nested product (`((a*b)+1)*c`), or the computed operand on the RIGHT (`b*(a-1)`)
// keep source order and were already exact.
int mul_varconst(int a, int b)  { return (a - 1) * b; }
int mul_constvar(int a, int b)  { return (2 - a) * b; }
int mul_negate(int a, int b)    { return -a * b; }
int and_varconst(int a, int b)  { return (a - 1) & b; }
int or_varconst(int a, int b)   { return (a - 1) | b; }
int xor_varconst(int a, int b)  { return (a - 1) ^ b; }
int mul_addconst(int a, int b)  { return (a + 1) * b; }
// The `const - variable` (subfic) left is computed-first in an ADD too.
int add_constvar(int a, int b)  { return (2 - a) + b; }
// Neighbors that keep source order (locked so the computed-first rule isn't over-applied):
int mul_computed_right(int a, int b)    { return b * (a - 1); }   // computed on the RIGHT
int mul_two_register(int a, int b, int c) { return (a + b) * c; } // two-register left
int mul_nested_product(int a, int b, int c) { return ((a * b) + 1) * c; }
