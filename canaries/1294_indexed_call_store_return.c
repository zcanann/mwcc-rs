// An index and two later arguments survive an inner call. The store and return
// reuse one scaled index and one global-array base.
// builds: GC/1.3.2
// flags: -O3,s -inline noauto -schedule off -use_lmw_stmw off -Cpp_exceptions off
static int identifiers[4];

// At -O3,s -inline noauto an unused plain-inline asm body is dropped without
// materializing the O4-era phantom global symbol.
inline int unused_inline_asm(int value)
{
    asm { addi r3, r3, 1 }
}

extern void* current_layer(void);
extern int request(void* layer, short process, void* callback, void* data, void* append);

int create_request(int index, short process, void* append)
{
    identifiers[index] = request(current_layer(), process, 0, 0, append);
    return identifiers[index];
}
