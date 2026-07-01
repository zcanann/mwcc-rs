// mwcc creates an IMPLICITLY-declared callee's symbol (K&R first-use, no prototype) at
// its call site INSIDE the function body, so it lands in the symbol table AFTER the
// function's own symbol. A prototyped/explicit external is created at its file-scope
// declaration and precedes the function. The ssbm dolphin/base PPCPm.c real-file DIFF:
// every callee there is implicitly declared, so mwcc emits `PMBegin` before its callees.
//   void f(void){ a(); }             symtab: f, a          (implicit callee after f)
//   extern void e(void); void g(void){ e(); }  symtab: e, g   (prototyped: before g)
//   void h(void){ e(); k(); }        symtab: e already listed; h; then implicit k
extern void e(void);

void impl_one(void)  { imp_a(); }                 // f, imp_a
void impl_two(void)  { imp_b(); imp_c(); imp_b(); }  // impl_two, imp_b, imp_c
void proto_call(void){ e(); }                     // e (already listed), proto_call
void mixed(void)     { e(); imp_d(); }            // e listed; mixed; then imp_d
