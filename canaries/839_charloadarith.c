// The char-member promotion fix generalizes to EVERY signed-char load — struct member
// (`p->x`), array element (`a[i]`, `a[2]`), and pointer dereference (`*p`) — via the
// is_signed_byte_load predicate (width-8 signed). All of them miss the extsb their
// `lbz`/`lbzx` does not carry when promoted to int, so as a direct operand of any
// non-truncating operator they DEFER (were miscompiles on the raw zero-extended byte).
//
// A SHORT load is the contrast that proves the rule: `lha`/`lhax` sign-extend ON LOAD, so a
// short member / array element / dereference in arithmetic is already correct and stays
// BYTE-EXACT — these must NOT defer.
short  gshort_arr[8];
struct SS { short y; };
int short_array(int i)          { return gshort_arr[i] + 1; }   // lhax (sign-extended); addi
int short_array_const(void)     { return gshort_arr[2] + 1; }   // lha; addi
int short_member(struct SS* p)  { return p->y + 1; }            // lha; addi
int short_member_div(struct SS* p) { return p->y / 2; }         // signed divide on lha value
int short_deref(short* p)       { return *p + 1; }              // lha; addi
// A strict partial mask of a signed char member stays byte-exact (the mask clears the
// would-be sign-extended bits): the exemption survives the generalization.
struct CC { char x; };
int char_member_mask(struct CC* p) { return p->x & 0xf; }       // lbz r0; clrlwi

// DEFERRED (signed char load as a direct integer operand — the extsb + r0 register choice is
// gated on the keystone allocator): p->x / a[i] / a[2] / *p  used with  + - * << >> | ^ / >
// < == != , or unary - ~ . The direct `return` stays byte-exact for a member / deref /
// pointer-index (the return path adds the extsb); only a global-array const index such as
// `return a[2]` defers, for the unrelated pre-existing "'a' is not a pointer" reason.
