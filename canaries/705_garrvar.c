// Variable-index access to a file-scope array global: scale the index, materialize
// the base, and `lwzx`. mwcc runs the scale before the base lands in the
// destination (the index register is often the destination); for a large array the
// base's high half goes to a register the scale won't clobber.
static int garrvar_tbl[8] = { 0, 1, 4, 9, 16, 25, 36, 49 };
int garrvar(int i) { return garrvar_tbl[i]; }
