// A union is laid out like a struct with EVERY member at offset 0 (overlapping storage) and a
// size equal to its largest member. Inline unions inside structs were already handled (the
// game's model structs overlay variant payloads); this adds the top-level `union Tag { … };`
// declaration and the `union Tag*` type, which reuse the struct machinery. Member access,
// store, float members, and indexed access are all byte-exact — identical to an offset-0
// struct member.
union U { int i; char c; float f; };
int   union_int(union U* p)          { return p->i; }       // lwz r3,0(p)
int   union_char(union U* p)         { return p->c; }       // lbz; extsb
float union_float(union U* p)        { return p->f; }       // lfs f1,0(p)
void  union_store(union U* p, int x) { p->i = x; }          // stw x,0(p)
int   union_index(union U* p, int j) { return p[j].i; }     // slwi; lwzx (size = 4)
union V { int a; int b; };
int   union_overlap(union V* p)      { return p->a + p->b; }// both at offset 0 (sum of the same word)
