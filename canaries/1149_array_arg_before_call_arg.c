// Extends the callee-saved argument-scheduler slice (canary 1148) to ARRAY first arguments —
// `rf(arr, g())`. The array's address materializes around the call-result copy, keyed by addressing:
//   small array (SDA21, total <= 8):  bl g; mr r4,r3; li r3,arr@sda21
//   large array (ADDR16):             bl g; lis r5,arr@ha; mr r4,r3; addi r3,r5,arr@l
// — the large form's `lis` fills the call-return latency slot BETWEEN the bl and the mr, through r5
// (the first register past both arguments). An UNSIZED extern array (`extern eti_t _eti[];`, size
// unknowable) now registers in the parser (previously "unknown variable") and addresses absolutely,
// like mwcc. This is the Runtime __init_cpp_exceptions shape
// `fragmentID = __register_fragment(_eti_init_info, GetR2());` including its guard. (fire 638)
typedef struct { void* a; void* b; } eti_t;
extern eti_t _eti[];
extern int rf(void*, void*);
extern void* r2(void);
void* big[4];
int small2[1];
static int fid = -2;
void aac_unsized(void)  { rf(_eti, r2()); }                          // bl; lis r5; mr r4,r3; addi r3,r5
void aac_large(void)    { rf(big, r2()); }                           // bl; lis r5; mr r4,r3; addi r3,r5
void aac_small(void)    { rf(small2, r2()); }                        // bl; mr r4,r3; li r3,small2@sda21
void aac_guarded(void)  { if (fid == -2) { fid = rf(_eti, r2()); } } // lwz;cmpwi;bne; bl; lis; mr; addi; bl; stw
