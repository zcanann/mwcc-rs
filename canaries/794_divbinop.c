// A constant-divide as one operand of a binary op must keep the *other* operand's
// register reserved while the divide's magic-number sequence runs, or the magic
// temporary clobbers it. `b + a/3` and `a/3 - b` are byte-exact this way (the divide's
// result lands in the scratch, the live leaf survives). The commutative form `a/3 + b`
// additionally needs mwcc's result-first operand order, which isn't modeled, so it
// defers rather than miscompile — verified separately.
int rs, rt, ru, rv;
void sub_md(int a, int b) { rs = a/3 - b; }   // magic divide, subtract: leaf reserved
void sub_p2(int a, int b) { rt = a/4 - b; }   // pow2 divide, subtract
void rev_md(int a, int b) { ru = b + a/3; }   // leaf + divide (false,true)
void rev_p2(int a, int b) { rv = a + b/4; }   // leaf + pow2 divide
