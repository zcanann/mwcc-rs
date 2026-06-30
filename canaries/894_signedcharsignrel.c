// Signed char load (deref/member/element) sign comparisons `> 0`, `>= 0`, `!= 0` against zero,
// extending canary 893 (`< 0`). Each keeps the sign-extended byte in mwcc's chosen register:
//   *p>0  : lbz r0; extsb r3,r0; neg r0,r3; andc r0,r0,r3; srwi r3,r0,31   (value in dest r3)
//   *p>=0 : lbz r0; extsb r0,r0; srwi r0,r0,31; xori r3,r0,1               (value in scratch r0)
//   *p!=0 : lbz r0; extsb r3,r0; neg r0,r3; or r0,r0,r3; srwi r3,r0,31     (value in dest r3)
// sign_idiom_source (used by >0 and !=0) now sign-extends a signed byte into the destination; the
// >=0 case loads into the scratch and extends in place; the comparison pre-check (expressions.rs)
// allows these four relations. `== 0` (cntlzw, would double-extend via place_operand) and `<= 0`
// (cntlzw+rlwnm) still DEFER, as do `== c`/`> c` and signed divide.
int gt0(char *p)              { return *p > 0; }
int ge0(char *p)              { return *p >= 0; }
int ne0(char *p)             { return *p != 0; }
struct S { char x; int y; };
int member_gt0(struct S *s)   { return s->x > 0; }
