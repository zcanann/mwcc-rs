// A call passing two WORD loads from one pointer base — `g(p[0], p[1])`, like `g(p->a, p->b)` —
// where evaluating the first argument clobbers the base register (p in r3). mwcc pre-copies the
// base to the second argument register, then loads each: `mr r4,r3; lwz r3,0(r3); lwz r4,4(r4)`.
// Without the pre-copy, the second load reads through the first's result — a MISCOMPILE (ours did
// this for subscripts; the member form `g(p->a, p->b)` already handled it). The base-preservation
// now covers constant-index word subscripts too; a narrow (short/char) element defers via the
// argument-clobber guard (collect_registers now sees a subscript/deref base register).
struct S { int a; int b; };
int g2(int, int);
int subscript_base(int* p)         { return g2(p[0], p[1]); }    // mr r4,r3; lwz r3,0(r3); lwz r4,4(r4)
int subscript_offsets(int* p)      { return g2(p[1], p[3]); }    // lwz r3,4; lwz r4,12
int member_base(struct S* s)       { return g2(s->a, s->b); }    // (member form still handled)
int distinct_bases(int* p, int* q) { return g2(p[0], q[0]); }    // different bases: no pre-copy needed
