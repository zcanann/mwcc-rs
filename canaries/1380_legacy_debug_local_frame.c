// builds: GC/1.2.5n
// flags: -sym on

extern void first(void*, void*);
extern void later(void*);

void legacy_debug_local_frame(void* pointer)
{
    void* first_local = *(void**) pointer;
    void* transient_local = *(void**) first_local;
    first(transient_local, first_local);
    later(pointer);
    later(first_local);
}
