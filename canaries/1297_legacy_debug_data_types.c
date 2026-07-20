// Functionless debug units retain scalar and array types, plus the source
// identity and ordered members of aggregates used by global definitions.
// builds: GC/2.6
// flags: -char unsigned -sdata 0 -sdata2 0 -O4,p -inline off -sym on

typedef unsigned char u8;
typedef short s16;

typedef struct animation_s {
    u8* flag_table;
    s16* data_table;
    s16* key_table;
    s16* fixed_table;
    s16 pad;
    s16 frames;
} Animation;

double scale = 0.5;
u8 flags[] = {0, 1};
s16 data[] = {10};
s16 keys[] = {1, 2, 3};
s16 fixed[] = {4, 5};
Animation animation = {flags, data, keys, fixed, -1, 11};
