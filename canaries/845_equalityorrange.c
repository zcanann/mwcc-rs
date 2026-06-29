// `(a == c1) || (a == c2)` as a VALUE: when c1 and c2 are CONSECUTIVE, mwcc recognizes the
// range and emits `(unsigned)(a - min) <= 1` as a branchless idiom (addi; subfic; orc; srwi;
// subf; srwi.; bnelr) — not reproduced yet, so that case DEFERS rather than emit our
// compare-branch form (a byte diff). Everything else here keeps the straightforward
// compare-branch lowering, which mwcc uses too, so it stays BYTE-EXACT:
int  or_nonconsecutive(int a)     { return a == 1 || a == 5; }   // not a range -> compare/branch
int  or_nonconsecutive2(int a)    { return a == 1 || a == 3; }
int  or_two_variables(int a,int b){ return a == 1 || b == 2; }   // different vars
int  or_relational(int a)         { return a < 1 || a > 2; }     // relational, not equality
int  or_plain(int a, int b)       { return a || b; }
int  and_consecutive(int a)       { return a == 1 && a == 2; }   // AND, not an OR range
int  cond_consecutive(int a)      { if (a == 1 || a == 2) return 9; return 0; } // CONDITION form

// DEFERRED (value form, consecutive constants -> mwcc unsigned range idiom):
//   int f(int a) { return a == 1 || a == 2; }
//   int f(int a) { return a == 0 || a == 1; }
