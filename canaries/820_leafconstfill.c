// Three or more stores to distinct integer SDA globals where exactly one value is a
// constant and the rest are register-resident leaf parameters (`gi=a; gj=b; gk=5;`).
// mwcc hoists the constant's `li` into the scratch up front and stores in source order —
// except a constant store cannot occupy the `li`'s one-cycle latency slot, so a LEADING
// constant swaps with the next (leaf) store:
//
//     gi=a; gj=b; gk=5  ->  li r0,5; stw r3,gi; stw r4,gj; stw r0,gk   (source order)
//     gi=5; gj=a; gk=b  ->  li r0,5; stw r3,gj; stw r0,gi; stw r4,gk   (leading const swaps)
//
// (Two stores are the mixed fill; all-constant is the constant fill; a leaf is kept in its
// own parameter register, so duplicated and reversed leaves work too. Cases with two
// constants or a computed value among the leaves need the general scheduler and defer.)
int gi, gj, gk, gl;
void const_last(int a, int b)         { gi = a; gj = b; gk = 5; }
void const_middle(int a, int b)       { gi = a; gj = 5; gk = b; }
void const_first(int a, int b)        { gi = 5; gj = a; gk = b; }   // leading const swaps off the slot
void four_const_last(int a,int b,int c){ gi=a; gj=b; gk=c; gl=9; }
void four_const_first(int a,int b,int c){ gi=9; gj=a; gk=b; gl=c; } // leading const swaps
void reversed_leaves(int a, int b)    { gi = b; gj = a; gk = 42; }
