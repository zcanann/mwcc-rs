// A 32-bit call result assigned through a 64-bit global into a 64-bit local.
// Although every tested mask fits in the low word, legacy optimizers may retain
// the source-wide value graph and perform full two-word bit tests.

typedef unsigned long long u64;

extern unsigned int read_inputs(void);

u64 input_snapshot;

int test_wide_call_result_masks(void)
{
    u64 events;

    events = input_snapshot = read_inputs();
    if (events & 0x20) {
        return 1;
    }
    if (events & 0x80) {
        return 2;
    }
    return 0;
}
