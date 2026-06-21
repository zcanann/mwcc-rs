// A bare global array variable in value position decays to its address (`return
// g;` for `int g[N]` returns &g, via `lis;addi` for a large array / `li@sda21`
// for a small one), not a load of g[0]. Struct arrays decay the same way.
int arraydecay_words[10];
struct ArrayDecayS { int a; int b; };
struct ArrayDecayS arraydecay_recs[4];
int *arraydecay_get_words(void) { return arraydecay_words; }
struct ArrayDecayS *arraydecay_get_recs(void) { return arraydecay_recs; }
