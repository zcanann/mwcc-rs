// `return !(X && Y)` and `return !(X || Y)` — mwcc applies De Morgan and folds the negation
// into the short-circuit exits (`!(a&&b)` == `!a || !b`) rather than computing the operator
// into a register and inverting it with cntlzw/srwi. evaluate_tail rewrites a tail
// `!(logical)` into the flipped-operator short-circuit of the negated terms:
//
//     return !(a && b);   ->   cmpwi r3,0; li r3,0; beq L; cmpwi r4,0; bnelr; L: li r3,1; blr
//
// Single level only: a nested logical operand (`!(a && b && c)`) defers. Non-tail negated
// logicals (`g = !(a&&b)`) and logicals as call arguments (`foo(a&&b)`) still defer/diff —
// they need a non-tail short-circuit that targets the destination directly (roadmap).
int not_and(int a, int b) { return !(a && b); }   // !a || !b
int not_or(int a, int b)  { return !(a || b); }   // !a && !b
