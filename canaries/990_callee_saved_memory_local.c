// A MEMORY-loaded local carried across calls in r31 (the signal.c handler fetch):
// a scalar global loads in the prologue -- `stwu; mflr; stw r0; stw r31; lwz r31,gi;
// bl; lwz r0; mr r3,r31; lwz r31; mtlr; addi; blr` (the `mr` rides between the LR and
// r31 reloads) -- and a computed-index global-array element interleaves its address
// build into the prologue: `stwu; mflr; lis r4; stw r0; slwi r0,i; addi r3,r4;
// stw r31; lwzx r31,r3,r0; bl; ...`. (Previously this SEMANTICALLY miscompiled: the
// load was folded past the call.)
int gi;
int arr[6];
extern void g(void);
extern void h(void);

int scalar_across_call(void)      { int t = gi; g(); return t; }
int element_across_call(int i)    { int t = arr[i]; g(); return t; }
int scalar_across_two_calls(void) { int t = gi; g(); h(); return t; }
