// builds: GC/1.1 GC/1.3 GC/1.3.2 GC/2.0 GC/2.5 GC/2.6 GC/2.7
// flags: -O4,p -inline auto -Cpp_exceptions off

extern unsigned char playback_flags;
extern void decode(void);

void narrow_mask_zero_call(void)
{
    if ((playback_flags & 1) == 0) {
        decode();
    }
}
