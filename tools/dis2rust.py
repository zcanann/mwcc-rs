# Transcribe the e_fmod objdump into Instruction:: pushes (fire 438).
import re, sys
lines = open(sys.argv[1]).read().splitlines()
POOL = {}
if len(sys.argv) > 2:
    for pl in open(sys.argv[2]).read().split():
        pass
    for pl in open(sys.argv[2]).read().splitlines():
        parts = pl.split()
        if len(parts)==2: POOL[parts[0]] = parts[1]
instrs = []   # (idx, mnemonic, ops, reloc_or_None)
reloc = {}
for ln in lines:
    m = re.match(r'^\s*([0-9a-f]+):\s+((?:[0-9a-f]{2} ){4})\s*(\S+)\s*(.*)$', ln)
    if m:
        idx = int(m.group(1),16)//4
        ops = m.group(4).split('<')[0].strip()
        instrs.append([idx, m.group(3), [o.strip() for o in ops.split(',')] if ops else []])
        continue
    r = re.match(r'^\s*([0-9a-f]+):\s+(R_PPC_\S+)\s+(\S+)', ln)
    if r:
        reloc[int(r.group(1),16)//4] = (r.group(2), r.group(3))
def R(x): return x.replace('r','')
def imm(x): return x
targets = set()
for idx, mn, ops in instrs:
    if mn in ("b","beq","bne","blt","bgt","bge","ble","bdnz","beqlr","blelr"):
        if ops and re.match(r'^[0-9a-f]+$', ops[0]):
            targets.add(int(ops[0],16)//4)
out = []
for idx, mn, ops in instrs:
    if idx in targets:
        out.append(f"        self.bind_label(labels[&{idx}]);")
    rl = reloc.get(idx)
    if rl and rl[0] == "R_PPC_EMB_SDA21":
        # a pooled constant load: lfd fD,0(0) + SDA21 @N -> load_double_constant
        if mn == "lfd" and rl[1] in POOL:
            out.append(f"        self.load_double_constant({ops[0][1:]}, 0x{POOL[rl[1]]});")
            continue
        out.append(f"        // UNHANDLED SDA21: {mn} {ops} -> {rl[1]}")
        continue
    if rl:
        kind = {"R_PPC_ADDR16_HA":"Addr16Ha","R_PPC_ADDR16_LO":"Addr16Lo"}[rl[0]]
        out.append(f'        self.record_relocation(RelocationKind::{kind}, "{rl[1]}");')
    def push(s): out.append(f"        self.output.instructions.push(Instruction::{s});")
    def bc(o,b_):
        t = int(ops[-1],16)//4
        out.append(f"        self.emit_branch_conditional_to({o}, {b_}, labels[&{t}]); // {mn}")
    if   mn=="stwu": push(f"StoreWordWithUpdate {{ s: {R(ops[0])}, a: 1, offset: {ops[1].split('(')[0]} }}")
    elif mn=="stfd": push(f"StoreFloatDouble {{ s: {ops[0][1:]}, a: 1, offset: {ops[1].split('(')[0]} }}")
    elif mn=="lfd":  push(f"LoadFloatDouble {{ d: {ops[0][1:]}, a: {ops[1].split('(')[1].rstrip(')')[1:]}, offset: {ops[1].split('(')[0]} }}")
    elif mn=="lfdx": push(f"LoadFloatDoubleIndexed {{ d: {ops[0][1:]}, a: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="lwz":  push(f"LoadWord {{ d: {R(ops[0])}, a: 1, offset: {ops[1].split('(')[0]} }}")
    elif mn=="stw":  push(f"StoreWord {{ s: {R(ops[0])}, a: 1, offset: {ops[1].split('(')[0]} }}")
    elif mn=="clrlwi": push(f"ClearLeftImmediate {{ a: {R(ops[0])}, s: {R(ops[1])}, clear: {ops[2]} }}")
    elif mn=="clrrwi": push(f"AndContiguousMask {{ a: {R(ops[0])}, s: {R(ops[1])}, begin: 0, end: {31-int(ops[2])} }}")
    elif mn=="rlwinm": push(f"RotateAndMask {{ a: {R(ops[0])}, s: {R(ops[1])}, shift: {ops[2]}, begin: {ops[3]}, end: {ops[4]} }}")
    elif mn=="slwi": push(f"ShiftLeftImmediate {{ a: {R(ops[0])}, s: {R(ops[1])}, shift: {ops[2]} }}")
    elif mn=="srwi": push(f"ShiftRightLogicalImmediate {{ a: {R(ops[0])}, s: {R(ops[1])}, shift: {ops[2]} }}")
    elif mn=="srawi": push(f"ShiftRightAlgebraicImmediate {{ a: {R(ops[0])}, s: {R(ops[1])}, shift: {ops[2]} }}")
    elif mn=="slw": push(f"ShiftLeftWord {{ a: {R(ops[0])}, s: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="srw": push(f"ShiftRightWord {{ a: {R(ops[0])}, s: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="sraw": push(f"ShiftRightAlgebraicWord {{ a: {R(ops[0])}, s: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="or":  push(f"Or {{ a: {R(ops[0])}, s: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="or.": push(f"OrRecord {{ a: {R(ops[0])}, s: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="xor": push(f"Xor {{ a: {R(ops[0])}, s: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="neg": push(f"Negate {{ d: {R(ops[0])}, a: {R(ops[1])} }}")
    elif mn=="add": push(f"Add {{ d: {R(ops[0])}, a: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="subf": push(f"SubtractFrom {{ d: {R(ops[0])}, a: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="subf.": push(f"SubtractFromRecord {{ d: {R(ops[0])}, a: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="subfic": push(f"SubtractFromImmediate {{ d: {R(ops[0])}, a: {R(ops[1])}, immediate: {ops[2]} }}")
    elif mn=="addi": push(f"AddImmediate {{ d: {R(ops[0])}, a: {R(ops[1])}, immediate: {ops[2]} }}")
    elif mn=="addis": push(f"AddImmediateShifted {{ d: {R(ops[0])}, a: {R(ops[1])}, immediate: {ops[2]} }}")
    elif mn=="li":  push(f"load_immediate({R(ops[0])}, {ops[1]})")
    elif mn=="lis": push(f"load_immediate_shifted({R(ops[0])}, {ops[1]})")
    elif mn=="oris": push(f"OrImmediateShifted {{ a: {R(ops[0])}, s: {R(ops[1])}, immediate: {ops[2]} }}")
    elif mn=="mr":  push(f"move_register({R(ops[0])}, {R(ops[1])})")
    elif mn=="cmpwi": push(f"CompareWordImmediate {{ a: {R(ops[0])}, immediate: {ops[1]} }}")
    elif mn=="cmpw": push(f"CompareWord {{ a: {R(ops[0])}, b: {R(ops[1])} }}")
    elif mn=="cmplw": push(f"CompareLogicalWord {{ a: {R(ops[0])}, b: {R(ops[1])} }}")
    elif mn=="mtctr": push(f"MoveToCountRegister {{ s: {R(ops[0])} }}")
    elif mn=="fmul": push(f"FloatMultiplyDouble {{ d: {ops[0][1:]}, a: {ops[1][1:]}, c: {ops[2][1:]} }}")
    elif mn=="fdiv": push(f"FloatDivideDouble {{ d: {ops[0][1:]}, a: {ops[1][1:]}, b: {ops[2][1:]} }}")
    elif mn=="fadd": push(f"FloatAddDouble {{ d: {ops[0][1:]}, a: {ops[1][1:]}, b: {ops[2][1:]} }}")
    elif mn=="fsub": push(f"FloatSubtractDouble {{ d: {ops[0][1:]}, a: {ops[1][1:]}, b: {ops[2][1:]} }}")
    elif mn=="fmadd": push(f"FloatMultiplyAddDouble {{ d: {ops[0][1:]}, a: {ops[1][1:]}, c: {ops[2][1:]}, b: {ops[3][1:]} }}")
    elif mn=="fmsub": push(f"FloatMultiplySubtractDouble {{ d: {ops[0][1:]}, a: {ops[1][1:]}, c: {ops[2][1:]}, b: {ops[3][1:]} }}")
    elif mn=="fnmsub": push(f"FloatNegativeMultiplySubtractDouble {{ d: {ops[0][1:]}, a: {ops[1][1:]}, c: {ops[2][1:]}, b: {ops[3][1:]} }}")
    elif mn=="fneg": push(f"FloatNegate {{ d: {ops[0][1:]}, b: {ops[1][1:]} }}")
    elif mn=="fabs": push(f"FloatAbsolute {{ d: {ops[0][1:]}, b: {ops[1][1:]} }}")
    elif mn=="fcmpo": push(f"FloatCompareOrdered {{ a: {ops[-2][1:]}, b: {ops[-1][1:]} }}")
    elif mn=="cmplwi": push(f"CompareLogicalWordImmediate {{ a: {R(ops[0])}, immediate: {ops[1]} }}")
    elif mn=="blr": push("BranchToLinkRegister")
    elif mn=="b":
        t=int(ops[0],16)//4
        out.append(f"        self.emit_branch_to(labels[&{t}]); // b")
    elif mn=="beq": bc(12,2)
    elif mn=="bne": bc(4,2)
    elif mn=="blt": bc(12,0)
    elif mn=="bge": bc(4,0)
    elif mn=="bgt": bc(12,1)
    elif mn=="ble": bc(4,1)
    elif mn=="bdnz": bc(16,0)
    else:
        out.append(f"        // UNHANDLED: {mn} {ops}")
print(f"// {len(instrs)} instructions; label targets: {sorted(targets)}")
print("\n".join(out))
