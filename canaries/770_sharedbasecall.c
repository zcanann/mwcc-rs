// Two word members of one pointer base passed as arguments — loading the first
// clobbers the base register — so mwcc pre-copies the base to the second argument
// register, then loads each member: `mr r4,r3; lwz r3,off0(r3); lwz r4,off1(r4)`,
// with the pre-copy hoisted into the non-leaf prologue slot. The hoist applies
// whether the call is a statement or the returned value.
struct Pair { int a, b; };
void sink2(int, int);
int  combine(int, int);
void pass_pair(struct Pair *p)   { sink2(p->a, p->b); }
int  return_pair(struct Pair *p) { return combine(p->a, p->b); }
struct Wide { unsigned x, y, z; };
void pass_wide(struct Wide *w)   { sink2(w->y, w->z); }
