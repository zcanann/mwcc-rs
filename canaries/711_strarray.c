// A `char` array initialized with a string literal stores the bytes plus a NUL
// terminator. An inferred length is strlen+1; an explicit length zero-fills the
// tail (or, if shorter than the string, drops the NUL). Small strings land in
// `.sdata` (`.sdata2` when const), larger ones in `.data`.
char strarray_inferred[] = "hello";
char strarray_sized[8] = "hi";
const char strarray_const[] = "ro";
