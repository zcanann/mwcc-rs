// A CONDITIONAL STORE back into the loaded element, with the local live across a call
// (the raise() handler-reset): the scaled index survives in its own register so the
// base/index pair is reused by the store --
//   lis r4; slwi r5,i,2; stw r0,20; addi r3,r4; stw r31,12; lwzx r31,r3,r5;
//   cmpwi r31,1; beq SKIP; li r0,0; stwx r0,r3,r5; SKIP: bl; lwz r0,20; mr r3,r31; ...
int garr[6];
extern void g(void);

int reset_unless_one(int i) { int t = garr[i]; if (t != 1) garr[i] = 0; g(); return t; }
int set_if_truthy(int i)    { int t = garr[i]; if (t) garr[i] = 7; g(); return t; }
