// Reduction of Super Mario Strikers src/Dolphin/ax/AXVPB.c:15-38,420.
// Source-only Strikers file; absent from its active inventory. Equivalent AX code is active in BfBB.
// Original tables and cycle-increment RHS; the u16 mixerCtrl field is a parameter.
// Frontier: repeated shifted/masked indices into defined static global tables.
// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned int u32;
typedef unsigned short u16;
static u32 __AXMainMixCycles[16] = {
    0x00000000, 0x000002F8, 0x000002F8,
	0x000005BE, 0x000002F8, 0x000005F0,
	0x000005F0, 0x000008B6, 0x00000000,
	0x000004F1, 0x000004F1, 0x000009A6,
	0x000004F1, 0x000009E2, 0x000009E2,
	0x00000E97
};

static u32 __AXAuxMixCycles[32] = {
    0x00000000, 0x000002F8, 0x000002F8,
	0x000005BE, 0x00000000, 0x000004F1,
	0x000004F1, 0x000009A6, 0x000002F8,
	0x000005F0, 0x000005F0, 0x000008B6,
	0x000002F8, 0x000007E9, 0x000007E9,
	0x00000C9E, 0x00000000, 0x000002F8,
	0x000002F8, 0x000005BE, 0x00000000,
	0x000004F1, 0x000004F1, 0x000009A6,
	0x000004F1, 0x000007E9, 0x000007E9,
	0x00000AAF, 0x000004F1, 0x000009E2,
	0x000009E2, 0x00000E97
};

u32 ax_mix_cycles(u16 mixerCtrl) {
    return __AXMainMixCycles[mixerCtrl & 0xF]
        + __AXAuxMixCycles[(mixerCtrl >> 4) & 0x1F]
        + __AXAuxMixCycles[(mixerCtrl >> 9) & 0x1F] + 0x8C;
}
