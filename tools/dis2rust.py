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
        word = int(m.group(2).replace(' ',''),16)
        mn = m.group(3)
        # objdump misreads Gekko psq_st under some dialects (xxsel etc.) —
        # decode psq_l/psq_st from the raw word instead.
        if (word >> 26) in (56, 60):
            mn = 'psq_l' if (word >> 26) == 56 else 'psq_st'
            fr, ra = (word >> 21) & 31, (word >> 16) & 31
            w_, i_ = (word >> 15) & 1, (word >> 12) & 7
            off = word & 0xfff
            instrs.append([idx, mn, [f'f{fr}', f'{off}(r{ra})', str(w_), str(i_)]])
            continue
        ops = m.group(4).split('<')[0].strip()
        instrs.append([idx, mn, [o.strip() for o in ops.split(',')] if ops else []])
        continue
    r = re.match(r'^\s*([0-9a-f]+):\s+(R_PPC_\S+)\s+(\S+)', ln)
    if r:
        reloc[int(r.group(1),16)//4] = (r.group(2), r.group(3))
STRINGS = {}
if len(sys.argv) > 3:
    for line in open(sys.argv[3]):
        parts = line.split()
        if len(parts) >= 2:
            STRINGS[parts[0]] = parts[1]
STR_EMITTED = {}
def string_intern_expr(name):
    # Emit the intern on first reference (creation order = mwcc's), reuse after.
    hexbytes = STRINGS[name]
    if hexbytes == "-":
        return "self.intern_string_literal(&[])"
    array = ", ".join(f"0x{hexbytes[i:i+2]}" for i in range(0, len(hexbytes), 2))
    return f"self.intern_string_literal(&[{array}])"
def R(x): return x.replace('r','')
def imm(x): return x
targets = set()
for idx, mn, ops in instrs:
    if mn in ("b","beq","bne","blt","bgt","bge","ble","bdnz","beqlr","blelr"):
        # a crN-qualified branch (`bne cr1,34`) carries the target LAST
        if ops and re.match(r'^[0-9a-f]+$', ops[-1]):
            targets.add(int(ops[-1],16)//4)
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
        if mn == "lwz" and rl[1] in POOL:
            out.append(f"        self.load_word_constant({R(ops[0])}, 0x{POOL[rl[1]][:8]});")
            continue
        # an SDA21 access to a NAMED small-data global (errno, a kept const
        # scalar): the reloc carries the symbol; base 0, offset 0.
        if mn == "lfd" and not rl[1].startswith("@"):
            target = re.sub(r'\$\d+$', '', rl[1])
            out.append(f'        self.record_relocation(RelocationKind::EmbSda21, "{target}");')
            out.append(f"        self.output.instructions.push(Instruction::LoadFloatDouble {{ d: {ops[0][1:]}, a: 0, offset: 0 }});")
            continue
        if mn in ("stw","lwz") and not rl[1].startswith("@"):
            # a function-scoped static displays as name$K — the writer keys the
            # local object on the RAW name and assigns K itself.
            target = re.sub(r'\$\d+$', '', rl[1])
            out.append(f'        self.record_relocation(RelocationKind::EmbSda21, "{target}");')
            kind = "StoreWord {{ s: {r}, a: 0, offset: 0 }}" if mn=="stw" else "LoadWord {{ d: {r}, a: 0, offset: 0 }}"
            out.append(f"        self.output.instructions.push(Instruction::{kind.format(r=R(ops[0]))});")
            continue
        # a short-string address: `li rD,0` + SDA21 @str -> the @@strN
        # placeholder the unit resolver rewrites (strings.rs model).
        if mn == "li" and rl[1] in STRINGS:
            out.append(f'        let index = {string_intern_expr(rl[1])};')
            out.append(f'        self.record_relocation(RelocationKind::EmbSda21, &format!("@@str{{index}}"));')
            out.append(f"        self.output.instructions.push(Instruction::AddImmediate {{ d: {R(ops[0])}, a: 0, immediate: 0 }});")
            continue
        out.append(f"        // UNHANDLED SDA21: {mn} {ops} -> {rl[1]}")
        continue
    if rl and rl[0] == "R_PPC_REL24":
        pass  # handled by the bl case itself
    elif rl:
        kind = {"R_PPC_ADDR16_HA":"Addr16Ha","R_PPC_ADDR16_LO":"Addr16Lo"}[rl[0]]
        if rl[1] in STRINGS:
            # a long (.data) string's address half — the @@strN placeholder.
            out.append(f'        let index = {string_intern_expr(rl[1])};')
            out.append(f'        self.record_relocation(RelocationKind::{kind}, &format!("@@str{{index}}"));')
        elif rl[1].startswith("@"):
            # a jump-table base (@N .data object) — the writer resolves it.
            out.append(f"        self.record_target(RelocationKind::{kind}, mwcc_machine_code::RelocationTarget::JumpTable);")
        else:
            target = re.sub(r'\$\d+$', '', rl[1])
            out.append(f'        self.record_relocation(RelocationKind::{kind}, "{target}");')
    def push(s): out.append(f"        self.output.instructions.push(Instruction::{s});")
    def bc(o,b_):
        t = int(ops[-1],16)//4
        # a crN-qualified branch (`bne cr1,X`) offsets the condition bit by 4*N
        if ops and ops[0].startswith("cr") and len(ops[0]) == 3:
            b_ = b_ + 4 * int(ops[0][2])
        out.append(f"        self.emit_branch_conditional_to({o}, {b_}, labels[&{t}]); // {mn}")
    if   mn=="stwu": push(f"StoreWordWithUpdate {{ s: {R(ops[0])}, a: {ops[1].split('(')[1].rstrip(')')[1:]}, offset: {ops[1].split('(')[0]} }}")
    elif mn=="stfd": push(f"StoreFloatDouble {{ s: {ops[0][1:]}, a: {ops[1].split('(')[1].rstrip(')')[1:]}, offset: {ops[1].split('(')[0]} }}")
    elif mn=="lfd":  push(f"LoadFloatDouble {{ d: {ops[0][1:]}, a: {ops[1].split('(')[1].rstrip(')')[1:]}, offset: {ops[1].split('(')[0]} }}")
    elif mn=="fctiwz": push(f"ConvertToIntegerWordZero {{ d: {ops[0][1:]}, b: {ops[1][1:]} }}")
    elif mn=="xoris": push(f"XorImmediateShifted {{ a: {R(ops[0])}, s: {R(ops[1])}, immediate: {ops[2]} }}")
    elif mn=="lfdx": push(f"LoadFloatDoubleIndexed {{ d: {ops[0][1:]}, a: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="lwz":  push(f"LoadWord {{ d: {R(ops[0])}, a: {ops[1].split('(')[1].rstrip(')')[1:]}, offset: {ops[1].split('(')[0]} }}")
    elif mn=="lwzu": push(f"LoadWordWithUpdate {{ d: {R(ops[0])}, a: {ops[1].split('(')[1].rstrip(')')[1:]}, offset: {ops[1].split('(')[0]} }}")
    elif mn=="lbzu": push(f"LoadByteZeroWithUpdate {{ d: {R(ops[0])}, a: {ops[1].split('(')[1].rstrip(')')[1:]}, offset: {ops[1].split('(')[0]} }}")
    elif mn=="mulli": push(f"MultiplyImmediate {{ d: {R(ops[0])}, a: {R(ops[1])}, immediate: {ops[2]} }}")
    elif mn=="add.": push(f"AddRecord {{ d: {R(ops[0])}, a: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="lfdu": push(f"LoadFloatDoubleWithUpdate {{ d: {ops[0][1:]}, a: {ops[1].split('(')[1].rstrip(')')[1:]}, offset: {ops[1].split('(')[0]} }}")
    elif mn=="stfdu": push(f"StoreFloatDoubleWithUpdate {{ s: {ops[0][1:]}, a: {ops[1].split('(')[1].rstrip(')')[1:]}, offset: {ops[1].split('(')[0]} }}")
    elif mn=="stfdx": push(f"StoreFloatDoubleIndexed {{ s: {ops[0][1:]}, a: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="srwi.": push(f"RotateAndMaskRecord {{ a: {R(ops[0])}, s: {R(ops[1])}, shift: {32-int(ops[2])}, begin: {ops[2]}, end: 31 }}")
    elif mn=="stw":  push(f"StoreWord {{ s: {R(ops[0])}, a: {ops[1].split('(')[1].rstrip(')')[1:]}, offset: {ops[1].split('(')[0]} }}")
    elif mn=="clrlwi": push(f"ClearLeftImmediate {{ a: {R(ops[0])}, s: {R(ops[1])}, clear: {ops[2]} }}")
    elif mn=="clrrwi": push(f"AndContiguousMask {{ a: {R(ops[0])}, s: {R(ops[1])}, begin: 0, end: {31-int(ops[2])} }}")
    elif mn=="rlwinm": push(f"RotateAndMask {{ a: {R(ops[0])}, s: {R(ops[1])}, shift: {ops[2]}, begin: {ops[3]}, end: {ops[4]} }}")
    elif mn=="slwi": push(f"ShiftLeftImmediate {{ a: {R(ops[0])}, s: {R(ops[1])}, shift: {ops[2]} }}")
    elif mn=="rotlwi": push(f"RotateAndMask {{ a: {R(ops[0])}, s: {R(ops[1])}, shift: {ops[2]}, begin: 0, end: 31 }}")
    elif mn=="srwi": push(f"ShiftRightLogicalImmediate {{ a: {R(ops[0])}, s: {R(ops[1])}, shift: {ops[2]} }}")
    elif mn=="srawi": push(f"ShiftRightAlgebraicImmediate {{ a: {R(ops[0])}, s: {R(ops[1])}, shift: {ops[2]} }}")
    elif mn=="slw": push(f"ShiftLeftWord {{ a: {R(ops[0])}, s: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="srw": push(f"ShiftRightWord {{ a: {R(ops[0])}, s: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="sraw": push(f"ShiftRightAlgebraicWord {{ a: {R(ops[0])}, s: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="or":  push(f"Or {{ a: {R(ops[0])}, s: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="or.": push(f"OrRecord {{ a: {R(ops[0])}, s: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="xor": push(f"Xor {{ a: {R(ops[0])}, s: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="neg": push(f"Negate {{ d: {R(ops[0])}, a: {R(ops[1])} }}")
    elif mn=="not": push(f"Nor {{ a: {R(ops[0])}, s: {R(ops[1])}, b: {R(ops[1])} }}")
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
    elif mn=="lbz": push(f"LoadByteZero {{ d: {R(ops[0])}, a: {ops[1].split('(')[1].rstrip(')')[1:]}, offset: {ops[1].split('(')[0]} }}")
    elif mn=="lhz": push(f"LoadHalfwordZero {{ d: {R(ops[0])}, a: {ops[1].split('(')[1].rstrip(')')[1:]}, offset: {ops[1].split('(')[0]} }}")
    elif mn=="lhzu": push(f"LoadHalfZeroWithUpdate {{ d: {R(ops[0])}, a: {ops[1].split('(')[1].rstrip(')')[1:]}, offset: {ops[1].split('(')[0]} }}")
    elif mn=="lha": push(f"LoadHalfwordAlgebraic {{ d: {R(ops[0])}, a: {ops[1].split('(')[1].rstrip(')')[1:]}, offset: {ops[1].split('(')[0]} }}")
    elif mn=="stb": push(f"StoreByte {{ s: {R(ops[0])}, a: {ops[1].split('(')[1].rstrip(')')[1:]}, offset: {ops[1].split('(')[0]} }}")
    elif mn=="sth": push(f"StoreHalfword {{ s: {R(ops[0])}, a: {ops[1].split('(')[1].rstrip(')')[1:]}, offset: {ops[1].split('(')[0]} }}")
    elif mn=="bltlr": push("BranchConditionalToLinkRegister { options: 12, condition_bit: 0 }")
    elif mn=="bgelr": push("BranchConditionalToLinkRegister { options: 4, condition_bit: 0 }")
    elif mn=="bnelr": push("BranchConditionalToLinkRegister { options: 4, condition_bit: 2 }")
    elif mn=="blelr": push("BranchConditionalToLinkRegister { options: 4, condition_bit: 1 }")
    elif mn=="addic.": push(f"AddImmediateCarryingRecord {{ d: {R(ops[0])}, a: {R(ops[1])}, immediate: {ops[2]} }}")
    elif mn=="and": push(f"And {{ a: {R(ops[0])}, s: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="and.": push(f"AndRecord {{ a: {R(ops[0])}, s: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="extsb": push(f"ExtendSignByte {{ a: {R(ops[0])}, s: {R(ops[1])} }}")
    elif mn=="extsh": push(f"ExtendSignHalfword {{ a: {R(ops[0])}, s: {R(ops[1])} }}")
    elif mn=="extsh.": push(f"ExtendSignHalfwordRecord {{ a: {R(ops[0])}, s: {R(ops[1])} }}")
    elif mn=="extsb.": push(f"ExtendSignByteRecord {{ a: {R(ops[0])}, s: {R(ops[1])} }}")
    elif mn=="cntlzw": push(f"CountLeadingZeros {{ a: {R(ops[0])}, s: {R(ops[1])} }}")
    elif mn=="stbu": push(f"StoreByteWithUpdate {{ s: {R(ops[0])}, a: {ops[1].split('(')[1].rstrip(')')[1:]}, offset: {ops[1].split('(')[0]} }}")
    elif mn=="beqlr": push("BranchConditionalToLinkRegister { options: 12, condition_bit: 2 }")
    elif mn=="bctrl": push("BranchToCountRegisterAndLink")
    elif mn=="bctr": push("BranchToCountRegister")
    elif mn=="mtctr": push(f"MoveToCountRegister {{ s: {R(ops[0])} }}")
    elif mn=="fmul": push(f"FloatMultiplyDouble {{ d: {ops[0][1:]}, a: {ops[1][1:]}, c: {ops[2][1:]} }}")
    elif mn=="fdiv": push(f"FloatDivideDouble {{ d: {ops[0][1:]}, a: {ops[1][1:]}, b: {ops[2][1:]} }}")
    elif mn=="fadd": push(f"FloatAddDouble {{ d: {ops[0][1:]}, a: {ops[1][1:]}, b: {ops[2][1:]} }}")
    elif mn=="fsub": push(f"FloatSubtractDouble {{ d: {ops[0][1:]}, a: {ops[1][1:]}, b: {ops[2][1:]} }}")
    elif mn=="fmadd": push(f"FloatMultiplyAddDouble {{ d: {ops[0][1:]}, a: {ops[1][1:]}, c: {ops[2][1:]}, b: {ops[3][1:]} }}")
    elif mn=="fmsub": push(f"FloatMultiplySubtractDouble {{ d: {ops[0][1:]}, a: {ops[1][1:]}, c: {ops[2][1:]}, b: {ops[3][1:]} }}")
    elif mn=="fnmsub": push(f"FloatNegativeMultiplySubtractDouble {{ d: {ops[0][1:]}, a: {ops[1][1:]}, c: {ops[2][1:]}, b: {ops[3][1:]} }}")
    elif mn=="fneg": push(f"FloatNegate {{ d: {ops[0][1:]}, b: {ops[1][1:]} }}")
    elif mn=="frsp": push(f"RoundToSingle {{ d: {ops[0][1:]}, b: {ops[1][1:]} }}")
    elif mn=="stfs": push(f"StoreFloatSingle {{ s: {ops[0][1:]}, a: {ops[1].split('(')[1].rstrip(')')[1:]}, offset: {ops[1].split('(')[0]} }}")
    elif mn=="subfze": push(f"SubtractFromZeroExtended {{ d: {R(ops[0])}, a: {R(ops[1])} }}")
    elif mn=="fabs": push(f"FloatAbsolute {{ d: {ops[0][1:]}, b: {ops[1][1:]} }}")
    elif mn=="fcmpo": push(f"FloatCompareOrdered {{ a: {ops[-2][1:]}, b: {ops[-1][1:]} }}")
    elif mn=="cmplwi": push(f"CompareLogicalWordImmediate {{ a: {R(ops[0])}, immediate: {ops[1]} }}")
    elif mn=="psq_l": push(f"PairedSingleQuantizedLoad {{ d: {ops[0][1:]}, a: {ops[1].split('(')[1].rstrip(')')[1:]}, offset: {ops[1].split('(')[0]}, w: {ops[2]}, i: {ops[3]} }}")
    elif mn=="psq_st": push(f"PairedSingleQuantizedStore {{ s: {ops[0][1:]}, a: {ops[1].split('(')[1].rstrip(')')[1:]}, offset: {ops[1].split('(')[0]}, w: {ops[2]}, i: {ops[3]} }}")
    elif mn=="lfs":  push(f"LoadFloatSingle {{ d: {ops[0][1:]}, a: {ops[1].split('(')[1].rstrip(')')[1:]}, offset: {ops[1].split('(')[0]} }}")
    elif mn=="fcmpu": push(f"FloatCompareUnordered {{ a: {ops[-2][1:]}, b: {ops[-1][1:]} }}")
    elif mn=="frsqrte": push(f"FloatReciprocalSqrtEstimate {{ d: {ops[0][1:]}, b: {ops[1][1:]} }}")
    elif mn=="srawi.": push(f"ShiftRightAlgebraicImmediateRecord {{ a: {R(ops[0])}, s: {R(ops[1])}, shift: {ops[2]} }}")
    elif mn=="mflr": push(f"MoveFromLinkRegister {{ d: {R(ops[0])} }}")
    elif mn=="mtlr": push(f"MoveToLinkRegister {{ s: {R(ops[0])} }}")
    elif mn=="cror":
        cr = {"lt":0,"gt":1,"eq":2,"so":3,"un":3}
        d_,a_,b_ = (cr.get(o, o) for o in ops)
        push(f"ConditionRegisterOr {{ d: {d_}, a: {a_}, b: {b_} }}")
    elif mn=="bl":
        name = None
        # the reloc line supplies the target name; ops[0] is the placeholder offset
        if idx in reloc and reloc[idx][0]=="R_PPC_REL24": name = reloc[idx][1]
        out.append(f'        self.record_relocation(RelocationKind::Rel24, "{name}");')
        out.append(f'        self.output.instructions.push(Instruction::BranchAndLink {{ target: "{name}".to_string() }});')
    elif mn=="andc": push(f"AndComplement {{ a: {R(ops[0])}, s: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="clrlwi.": push(f"ClearLeftImmediateRecord {{ a: {R(ops[0])}, s: {R(ops[1])}, clear: {ops[2]} }}")
    elif mn=="rlwinm.": push(f"RotateAndMaskRecord {{ a: {R(ops[0])}, s: {R(ops[1])}, shift: {ops[2]}, begin: {ops[3]}, end: {ops[4]} }}")
    elif mn=="fmr": push(f"FloatMove {{ d: {ops[0][1:]}, b: {ops[1][1:]} }}")
    elif mn=="rlwimi": push(f"RotateAndMaskInsert {{ a: {R(ops[0])}, s: {R(ops[1])}, shift: {ops[2]}, begin: {ops[3]}, end: {ops[4]} }}")
    elif mn=="clrrwi.": push(f"AndMaskRecord {{ a: {R(ops[0])}, s: {R(ops[1])}, begin: 0, end: {31-int(ops[2])} }}")
    elif mn=="andi.": push(f"AndImmediateRecord {{ a: {R(ops[0])}, s: {R(ops[1])}, immediate: {ops[2]} }}")
    elif mn=="divwu": push(f"DivideWordUnsigned {{ d: {R(ops[0])}, a: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="divw": push(f"DivideWord {{ d: {R(ops[0])}, a: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="mullw": push(f"MultiplyLow {{ d: {R(ops[0])}, a: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="mullw.": push(f"MultiplyLowRecord {{ d: {R(ops[0])}, a: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="mulhwu": push(f"MultiplyHighWordUnsigned {{ d: {R(ops[0])}, a: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="mulhw": push(f"MultiplyHighWord {{ d: {R(ops[0])}, a: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="addc": push(f"AddCarrying {{ d: {R(ops[0])}, a: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="adde": push(f"AddExtended {{ d: {R(ops[0])}, a: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="addze": push(f"AddToZeroExtended {{ d: {R(ops[0])}, a: {R(ops[1])} }}")
    elif mn=="subfc": push(f"SubtractFromCarrying {{ d: {R(ops[0])}, a: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="subfe": push(f"SubtractFromExtended {{ d: {R(ops[0])}, a: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="lbzx": push(f"LoadByteZeroIndexed {{ d: {R(ops[0])}, a: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="lwzx": push(f"LoadWordIndexed {{ d: {R(ops[0])}, a: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="lhzx": push(f"LoadHalfwordZeroIndexed {{ d: {R(ops[0])}, a: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="stwx": push(f"StoreWordIndexed {{ s: {R(ops[0])}, a: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="stbx": push(f"StoreByteIndexed {{ s: {R(ops[0])}, a: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="sthx": push(f"StoreHalfwordIndexed {{ s: {R(ops[0])}, a: {R(ops[1])}, b: {R(ops[2])} }}")
    elif mn=="ori": push(f"OrImmediate {{ a: {R(ops[0])}, s: {R(ops[1])}, immediate: {ops[2]} }}")
    elif mn=="xori": push(f"XorImmediate {{ a: {R(ops[0])}, s: {R(ops[1])}, immediate: {ops[2]} }}")
    elif mn=="mr.": push(f"OrRecord {{ a: {R(ops[0])}, s: {R(ops[1])}, b: {R(ops[1])} }}")
    elif mn=="neg.": push(f"NegateRecord {{ d: {R(ops[0])}, a: {R(ops[1])} }}")
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
