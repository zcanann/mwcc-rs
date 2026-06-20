// An EXTERN array/struct-array global is subscriptable and member-accessible:
// mwcc addresses it identically to a defined one (SDA21 small / ADDR16 large),
// referencing it through a relocation to the undefined symbol. (The marioparty4
// game code reaches its global state tables — GWPlayer[player].field — this way,
// declared `extern` in shared headers.)
typedef struct { int first; int second; } ExternRec;
extern ExternRec extarr_table[16];
extern int extarr_words[16];
int extarr_member(int i) { return extarr_table[i].second; }
int extarr_word(int i)   { return extarr_words[i]; }
