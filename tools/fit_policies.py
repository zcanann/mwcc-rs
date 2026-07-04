#!/usr/bin/env python3
"""Fitting pass over the fixture corpus: derive value webs from the physical
register stream of STRAIGHT-LINE functions (no branches except the trailing
blr) and test whether simple assignment policies reproduce the numbers."""
import os, sys, glob

FIXDIR = sys.argv[1] if len(sys.argv) > 1 else 'fixtures'
ALL_FUNCTIONS = '--all' in sys.argv
BRANCHY = {'BranchConditionalForward','Branch','BranchAndLink','BranchToCountRegisterAndLink',
           'BranchConditionalToLinkRegister','BranchToCountRegister'}

def parse(path):
    lines = open(path).read().strip().split('\n')
    head = lines[0].split()
    name = head[1]
    instrs = []
    for line in lines[1:]:
        left, *ops = line.split(' | ')
        parts = left.split()
        idx, mnem = int(parts[0]), parts[1]
        call = 'CALL' in parts
        operands = []
        for op in ops:
            if not op.strip(): continue
            r, c, reg = op.split()
            operands.append((r, c, int(reg)))
        instrs.append((idx, mnem, call, operands))
    return name, instrs

def straight_line(instrs):
    for _, mnem, _, _ in instrs[:-1]:
        if mnem in BRANCHY or mnem == 'BranchToLinkRegister':
            return False
    return True

def derive_values(instrs):
    """Linear web derivation: DEF starts a value; USE extends the current one.
    A register USED before any def is an incoming PARAMETER — pre-seeded as a
    value defined at -1 (it occupies its register from entry)."""
    current = {}   # (class, reg) -> value id
    values = []    # id -> dict(class, reg, def_idx, last_use, param)
    for idx, mnem, call, ops in instrs:
        for role, cls, reg in ops:
            if role == 'U':
                key = (cls, reg)
                if key not in current:
                    vid = len(values)
                    values.append({'class': cls, 'reg': reg, 'def': -1, 'last': idx, 'param': True})
                    current[key] = vid
                values[current[key]]['last'] = idx
        for role, cls, reg in ops:
            if role == 'D':
                key = (cls, reg)
                vid = len(values)
                values.append({'class': cls, 'reg': reg, 'def': idx, 'last': idx, 'param': False})
                current[key] = vid
    return values

def predict(values, n_params, policy):
    """Assign registers to values in def order under `policy`; count matches.
    Params occupy r3.. at entry (value with def at the param-copy...) — for
    simplicity: values defined at idx<0 don't exist; we just simulate pools."""
    free_g = policy['pool'][:]         # available volatile GPRs in preference order
    active = []                        # (last, reg)
    hits = misses = 0
    for v in values:
        if v['class'] != 'G':
            continue
        if v.get('param'):
            # a parameter owns its ABI register until it dies — remove from the pool
            if v['reg'] in free_g:
                free_g.remove(v['reg'])
            active.append((v['last'], v['reg']))
            continue
        # expire
        for (last, reg) in active[:]:
            if last < v['def']:
                active.remove((last, reg))
                if reg in policy['pool'] and reg not in free_g:
                    # return to pool at its preference position
                    free_g.append(reg)
                    free_g.sort(key=lambda r: policy['pool'].index(r))
        if not free_g:
            misses += 1
            continue
        chosen = free_g.pop(0)
        if chosen == v['reg']:
            hits += 1
        else:
            misses += 1
            # resync: claim the ACTUAL register so later predictions aren't cascaded
            if v['reg'] in free_g:
                free_g.remove(v['reg'])
            if chosen not in free_g:
                free_g.insert(0, chosen)
        active.append((v['last'], v['reg']))
    return hits, misses

total_fns = straight = 0
agg = {}
POLICIES = {
    'r3-up':    {'pool': list(range(3, 13)) + [0]},
    'r0-first': {'pool': [0] + list(range(3, 13))},
    'r3-up-no0':{'pool': list(range(3, 13))},
}
for path in sorted(glob.glob(os.path.join(FIXDIR, '*.fixture'))):
    name, instrs = parse(path)
    total_fns += 1
    if len(instrs) < 4:
        continue
    if not straight_line(instrs):
        if not ALL_FUNCTIONS:
            continue
    else:
        straight += 1
    values = derive_values(instrs)
    for pname, pol in POLICIES.items():
        h, m = predict(values, 0, pol)
        a = agg.setdefault(pname, [0, 0])
        a[0] += h; a[1] += m
print(f"fixtures: {total_fns}, straight-line (>=4 instrs): {straight}")
for pname, (h, m) in agg.items():
    pct = 100*h/(h+m) if h+m else 0
    print(f"  policy {pname:10s}: {h}/{h+m} values predicted ({pct:.1f}%)")
