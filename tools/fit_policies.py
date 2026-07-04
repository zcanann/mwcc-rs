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

def call_indices(instrs):
    return [idx for idx, _, call, _ in instrs if call]

def predict_split(values, calls, policy):
    """Two-pool policy: values crossing a CALL draw from the SAVED pool,
    others from the VOLATILE pool. Returns per-pool (hits, total)."""
    free_v = policy['volatile'][:]
    free_s = policy['saved'][:]
    active = []  # (last, reg, pool_tag)
    score = {'v': [0, 0], 's': [0, 0]}
    for v in values:
        if v['class'] != 'G':
            continue
        # expire
        for entry in active[:]:
            last, reg, tag = entry
            if last < v['def']:
                active.remove(entry)
                pool, order = (free_v, policy['volatile']) if tag == 'v' else (free_s, policy['saved'])
                if reg in order and reg not in pool:
                    pool.append(reg)
                    pool.sort(key=lambda r: order.index(r))
        crosses = any(v['def'] < c < v['last'] for c in calls)
        tag = 's' if crosses else 'v'
        if v.get('param'):
            for pool in (free_v, free_s):
                if v['reg'] in pool:
                    pool.remove(v['reg'])
            active.append((v['last'], v['reg'], 'v'))
            continue
        pool = free_s if tag == 's' else free_v
        score[tag][1] += 1
        if pool:
            chosen = pool.pop(0)
            if chosen == v['reg']:
                score[tag][0] += 1
            else:
                if v['reg'] in pool:
                    pool.remove(v['reg'])
                pool.insert(0, chosen)
                pool.sort(key=lambda r: (policy['saved'] if tag == 's' else policy['volatile']).index(r))
        active.append((v['last'], v['reg'], tag))
    return score

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
agg_split = {}
SPLIT_POLICIES = {
    'v:r0.3-12/s:31down': {'volatile': [0] + list(range(3, 13)), 'saved': list(range(31, 13, -1))},
    'v:r0.3-12/s:14up':   {'volatile': [0] + list(range(3, 13)), 'saved': list(range(14, 32))},
    'v:r3up.0/s:31down':  {'volatile': list(range(3, 13)) + [0], 'saved': list(range(31, 13, -1))},
}
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
    calls = call_indices(instrs)
    for pname, pol in POLICIES.items():
        h, m = predict(values, 0, pol)
        a = agg.setdefault(pname, [0, 0])
        a[0] += h; a[1] += m
    for pname, pol in SPLIT_POLICIES.items():
        sc = predict_split(values, calls, pol)
        a = agg_split.setdefault(pname, {'v': [0, 0], 's': [0, 0]})
        for tag in ('v', 's'):
            a[tag][0] += sc[tag][0]; a[tag][1] += sc[tag][1]
print(f"fixtures: {total_fns}, straight-line (>=4 instrs): {straight}")
for pname, (h, m) in agg.items():
    pct = 100*h/(h+m) if h+m else 0
    print(f"  policy {pname:10s}: {h}/{h+m} values predicted ({pct:.1f}%)")
for pname, sc in agg_split.items():
    vh, vt = sc['v']; sh, st = sc['s']
    tot_h, tot_t = vh+sh, vt+st
    print(f"  split  {pname:22s}: volatile {vh}/{vt} ({100*vh/max(vt,1):.1f}%)  saved {sh}/{st} ({100*sh/max(st,1):.1f}%)  overall {100*tot_h/max(tot_t,1):.1f}%")
