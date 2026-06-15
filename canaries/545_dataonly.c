// A translation unit with no functions, only a global (MSL_C errno.c). The object
// omits .text and the .mwcats machinery entirely — just the data section and the
// symbol/string tables.
int errno;
