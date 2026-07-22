// builds: GC/1.2.5n
// flags: -Cpp_exceptions off -multibyte -pragma "cats off"

/* A mixed GPR/FPR call followed by another use of the pointer.  Legacy MWCC
 * schedules integer constants across the linkage prologue and reuses the first
 * zero-valued FPR for the third floating argument. */
extern void mixed_sink(void*, int, int, float, float, float, long);
extern void use_pointer(void*);

void legacy_mixed_call_schedule(void* pointer)
{
    mixed_sink(pointer, 289, 144, 0.0f, 1.0f, 0.0f, 0L);
    use_pointer(pointer);
}
