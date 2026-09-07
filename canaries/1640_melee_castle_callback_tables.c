// Reduced from Melee src/melee/gr/grcastle.c: both original callback tables.
// Opaque parameter types replace project headers; function bodies are omitted.
// flags: -Cpp_exceptions off -pragma "cats off"
struct unkCastle;
typedef struct unkCastle unkCastle;
typedef struct HSD_GObj Ground_GObj;
typedef void* UNK_T;
typedef void (*unkCastleCallback)(void*, struct unkCastle*);
typedef void (*unkCastleCallback2)(void*, struct unkCastle*, Ground_GObj*);
void grCastle_801D0550(UNK_T, unkCastle*);
void grCastle_801D059C(UNK_T, unkCastle*);
void grCastle_801D05E8(UNK_T, unkCastle*);
void grCastle_801D0634(UNK_T, unkCastle*);
void grCastle_801D0680(UNK_T, unkCastle*);
void grCastle_801D06CC(UNK_T, unkCastle*, Ground_GObj*);
void grCastle_801D0744(UNK_T, unkCastle*, Ground_GObj*);
void grCastle_801D07BC(UNK_T, unkCastle*, Ground_GObj*);
void grCastle_801D0834(UNK_T, unkCastle*, Ground_GObj*);
void grCastle_801D08AC(UNK_T, unkCastle*, Ground_GObj*);

const unkCastleCallback grCs_803B7F28[5] = {
    grCastle_801D0550, grCastle_801D059C, grCastle_801D05E8,
    grCastle_801D0634, grCastle_801D0680,
};

const unkCastleCallback2 grCs_803B7F3C[5] = {
    grCastle_801D06CC, grCastle_801D0744, grCastle_801D07BC,
    grCastle_801D0834, grCastle_801D08AC,
};
