// builds: GC/1.1p1
// flags: -Cpp_exceptions off -sdata 0 -sdata2 0

extern void copy_bytes(void* destination, const void* source, unsigned long size);
extern void release_handle(int handle);

void legacy_plain_copy(void* destination, const void* source)
{
    copy_bytes(destination, source, 12);
}

void legacy_plain_member_call(const int* event)
{
    release_handle(event[2]);
}
