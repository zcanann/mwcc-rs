// A leaf void `if (c) A; else B;`. mwcc has two forms by target kind:
//
//   * GLOBAL-store arms with a truthy condition use the re-test idiom — the then-arm
//     falls through to a *re-test* of the condition and a conditional return, then the
//     else, with no blr after the then-arm and no unconditional branch over the else:
//         cmpwi r3,0; beq L; <A>; L: cmpwi r3,0; bnelr; <B>; blr
//     (Was a diff: ours emitted the two-exit form for these.)
//
//   * MEMBER / base-register arms keep the two-exit form — the then-arm returns, the
//     conditional branch lands on the else:
//         cmpwi; beq L; <A>; blr; L: <B>; blr
//
// A comparison condition (`if (a>b) ...`) re-tests by branchless recomputation rather
// than a second compare — a separate idiom, still deferred (two-exit), not regressed.
int gi, gj;
struct S { int x, y; };
void g_const(int c)            { if (c) gi = 1; else gj = 2; }     // re-test (global)
void g_var(int c, int a, int b){ if (c) gi = a; else gj = b; }     // re-test (global)
void g_neg(int c)              { if (!c) gi = 1; else gj = 2; }    // re-test, negated cond
void m_member(struct S *p, int c) { if (c) p->x = 1; else p->y = 2; }  // two-exit (member)
