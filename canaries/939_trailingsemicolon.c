// Stray empty statements (`;`) may trail the return before the closing brace (`return x;;`, or a lone
// `;` after the last statement) — they produce no code. The body parser skips them before expecting
// `}`, rather than failing with "expected BraceClose, found Semicolon". (Empty statements WITHIN the
// statement list were already skipped.)
void sink(int);
int  ret_double_semi(int a) { return a * 2;; }
void call_then_semi(int a)  { sink(a);; }
int  extra_empty(int a)     { return a + 1; ; }
