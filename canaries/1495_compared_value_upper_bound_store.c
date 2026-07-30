// builds: GC/1.2.5n
// flags: -O4,p -inline auto -Cpp_exceptions off -pragma "cats off"

typedef unsigned int u32;

typedef struct Transfer {
    u32 length;
    u32 transferred;
    u32 current;
} Transfer;

void observe(Transfer*);

void compared_value_upper_bound_store(Transfer* transfer)
{
    observe(transfer);
    transfer->current =
        (transfer->length - transfer->transferred > 0x80000)
            ? 0x80000
            : (transfer->length - transfer->transferred);
}
