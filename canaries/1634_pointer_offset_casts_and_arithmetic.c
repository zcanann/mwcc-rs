// Casts change subsequent pointer scaling, without rescaling an existing address.
// flags: -Cpp_exceptions off -pragma "cats off"
int words[16];
int* folded = words + (1 << 2) - 2;
int* commuted = 3 + words;
char* byte_offset = (char*)words + 3;
int* preserved_offset = (int*)((char*)words + 3);
int* backwards = &words[5] - 2;
int* negative = &words[0] - 1;
int* cancelled = words + 3 - 3;
