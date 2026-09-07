// BfBB src/dolphin/src/ax/AXVPB.c:697-699, configured for GC/1.2.5n.
// Original tables and complete three-lookup cycle expression; mixerCtrl remains u16.
// Fresh compiler-reference comparison pending; original DOL verifies the full expression.
// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned int u32;
typedef unsigned short u16;
static u32 __AXMainMixCycles[16] = { 0x00000000, 0x000002F8, 0x000002F8, 0x000005BE,
                                     0x000002F8, 0x000005F0, 0x000005F0, 0x000008B6,
                                     0x00000000, 0x000004F1, 0x000004F1, 0x000009A6,
                                     0x000004F1, 0x000009E2, 0x000009E2, 0x00000E97 };
static u32 __AXAuxMixCycles[32] = {
    0x00000000, 0x000002F8, 0x000002F8, 0x000005BE, 0x00000000, 0x000004F1, 0x000004F1, 0x000009A6,
    0x000002F8, 0x000005F0, 0x000005F0, 0x000008B6, 0x000002F8, 0x000007E9, 0x000007E9, 0x00000C9E,
    0x00000000, 0x000002F8, 0x000002F8, 0x000005BE, 0x00000000, 0x000004F1, 0x000004F1, 0x000009A6,
    0x000004F1, 0x000007E9, 0x000007E9, 0x00000AAF, 0x000004F1, 0x000009E2, 0x000009E2, 0x00000E97
};
u32 ax_accumulate_cycles(u16 mixerCtrl, u32 cycles) { return cycles + __AXMainMixCycles[mixerCtrl & 15] + __AXAuxMixCycles[(mixerCtrl >> 4) & 31] + __AXAuxMixCycles[(mixerCtrl >> 9) & 31] + 140; }
