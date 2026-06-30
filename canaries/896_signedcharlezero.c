// Signed char load `<= 0` — the last of the six zero sign-relations (893 <0, 894 >0/>=0/!=0, 895
// ==0). mwcc keeps the value in the scratch, sign-extends in place, and places the `1` in the
// destination between the load and the extend: `lbz r0; li r3,1; extsb r0,r0; cntlzw r0,r0;
// rlwnm r3,r3,r0,31,31`. comparisons.rs <=0 case adds a signed-byte branch for that exact order;
// the comparison pre-check (expressions.rs) now allows all six relations against zero for a signed
// char. With this, signed-char zero comparisons (< > >= <= == !=) are all byte-exact. The CONSTANT
// comparisons (`== c`, `> c`) and signed divide still DEFER (not DIFF).
int le0(char *p)             { return *p <= 0; }
struct S { char x; int y; };
int member_le0(struct S *s)  { return s->x <= 0; }
int elem_le0(char *a)        { return a[1] <= 0; }
