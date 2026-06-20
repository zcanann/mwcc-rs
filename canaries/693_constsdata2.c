// A small `const` file-scope global is read-only and lands in `.sdata2` (the
// same section as the float constant pool), not the writable `.sdata`. An
// integer scalar gets element alignment; the symbol is a GLOBAL object.
const int constsdata2 = 5;
