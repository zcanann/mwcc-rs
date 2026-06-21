// A `static` (file-local) function is emitted with a LOCAL STT_FUNC symbol, in
// declaration order right after the static data and before any unwind @N entries
// (mwcc places `static` functions ahead of global ones). Global functions keep
// their global symbol. (A small static function that is CALLED is inlined by mwcc
// — a separate feature; these are emitted because unused/address-distinct.)
static int staticfunc_a(int x) { return x + 1; }
static int staticfunc_b(int x) { return x + 2; }
int staticfunc_g = 5;
void staticfunc_pub(void) {}
