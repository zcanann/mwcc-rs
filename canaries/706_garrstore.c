// Storing to a file-scope array global. A constant index materializes the base
// (SDA21 small / ADDR16 large) into a free register, then a displacement store;
// a variable index scales into the scratch, lands the base in the freed index
// register, and `stwx`es the value (the large array's base high half avoids both
// the index and the value register). Register-valued stores.
int garrstore_small[2];
static int garrstore_large[4];
void garrstore_cv(int v) { garrstore_small[1] = v; }
void garrstore_ci(int v) { garrstore_large[2] = v; }
void garrstore_vv(int i, int v) { garrstore_large[i] = v; }
