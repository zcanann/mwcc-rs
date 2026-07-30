// builds: GC/1.2.5n
// flags: -O4,p -inline auto -Cpp_exceptions off -pragma "cats off"

typedef unsigned int u32;
typedef void (*Callback)(u32);

typedef struct Transfer {
    char* address;
    u32 transferred;
    u32 current;
    u32 offset;
} Transfer;

extern void read_async(char*, u32, u32, Callback);
extern void complete(u32);

void repeated_member_call_argument_reuse(Transfer* transfer)
{
    read_async(
        transfer->address + transfer->transferred,
        transfer->current,
        transfer->offset + transfer->transferred,
        complete);
}
