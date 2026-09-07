// BfBB AX lookup reduction; candidate execution compared with the linked mixer block.
// Fresh reference-compiler objects pending.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
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
struct Voice { unsigned pad[81]; u16 mixerCtrl; };
extern struct Voice* get_node(void);
u32 ax_accumulate_cycles(u32 cycles) { struct Voice* node; node = get_node(); return cycles + __AXMainMixCycles[node->mixerCtrl & 15] + __AXAuxMixCycles[(node->mixerCtrl >> 4) & 31] + __AXAuxMixCycles[(node->mixerCtrl >> 9) & 31] + 140; }
