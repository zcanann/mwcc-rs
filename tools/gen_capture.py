#!/usr/bin/env python3
"""Generate a src/captures/<name>.rs scaffold from a capture directory.

Usage: gen_capture.py <name> <fn_name> <capture_dir> <return_type> <n_params> \
       [--frame N] [--non-leaf] [--csf N] [--gap i:n] [--gate-skipped name] \
       [--gate-not-skipped name] [--fire N]
The AST hash and @N bump are left as dev-loop placeholders (hash 0 prints the
candidate; bump 0). Wire into mod.rs manually (one mod line, one dispatcher arm).
"""
import argparse, subprocess, sys, os, re
ap = argparse.ArgumentParser()
ap.add_argument('name'); ap.add_argument('fn_name'); ap.add_argument('dir')
ap.add_argument('return_type'); ap.add_argument('n_params', type=int)
ap.add_argument('--frame', type=int, default=0)
ap.add_argument('--non-leaf', action='store_true')
ap.add_argument('--csf', type=int, default=0)
ap.add_argument('--gap', action='append', default=[])
ap.add_argument('--gate-skipped', action='append', default=[])
ap.add_argument('--gate-not-skipped', action='append', default=[])
ap.add_argument('--fire', default='?')
a = ap.parse_args()

dis_args = [sys.executable, os.path.join(os.path.dirname(__file__), 'dis2rust.py'),
            os.path.join(a.dir, 'real.dis'), os.path.join(a.dir, 'pool.txt')]
if os.path.exists(os.path.join(a.dir, 'strings.txt')):
    dis_args.append(os.path.join(a.dir, 'strings.txt'))
emit = subprocess.run(dis_args, capture_output=True, text=True).stdout.splitlines()
targets = emit[0].split('label targets: ')[1]
body = "\n".join(emit[1:])
assert 'UNHANDLED' not in body, "dis2rust has unhandled instructions"
pool = [l.split()[1] for l in open(os.path.join(a.dir, 'pool.txt'))]
consts = ",\n            ".join(f"0x{b}" + ("u64" if i == 0 else "") for i, b in enumerate(pool))
upper = a.name.upper()
gates = ""
for n in a.gate_skipped:
    gates += f'\n            || !self.skipped_inline_names.contains("{n}")'
for n in a.gate_not_skipped:
    gates += f'\n            || self.skipped_inline_names.contains("{n}")'
setup = f"        self.frame_size = {a.frame};\n" if a.frame else ""
if a.non_leaf: setup += "        self.non_leaf = true;\n"
if a.csf: setup += f"        self.callee_saved_float = {a.csf};\n"
if a.gap:
    pairs = ", ".join(f"({g.split(':')[0]}, {g.split(':')[1]})" for g in a.gap)
    setup += f"        self.output.constant_number_gaps = vec![{pairs}];\n"
# PIN the external symbol order to the authoritative .text reference order (first-seen
# relocation targets, minus @N pool refs). The generator's AST fallback
# (symbol_order::referenced_names) mis-orders an ADDRESS-TAKEN external/callback (named
# early in source, referenced late in .text) — pinning the reloc order matches mwcc.
# Only affects THIS capture; verify byte-exact as always.
_reloc_names, _seen = [], set()
for _m in re.finditer(r'record_relocation(?:_with_addend)?\([^,]+,\s*"([^"]+)"', body):
    _n = _m.group(1)
    if not _n.startswith('@') and _n not in _seen:
        _seen.add(_n); _reloc_names.append(_n)
if _reloc_names:
    _names = ", ".join(f'"{n}"' for n in _reloc_names)
    setup += f"        self.output.symbol_order = [{_names}].into_iter().map(String::from).collect();\n"
pool_block = f"""        for bits in [
            {consts},
        ] {{
            self.output.intern_constant(bits, 8);
        }}
""" if pool else ""
out = f'''//! {a.name}: an exact-match whole-function capture (fire {a.fire}).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{{Instruction, RelocationKind}};
use mwcc_syntax_trees::{{Function, Type}};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const {upper}_AST_HASH: u64 = 0;

impl Generator {{
    pub(super) fn try_{a.name}(&mut self, function: &Function) -> Compilation<bool> {{
        if function.name != "{a.fn_name}"
            || function.return_type != Type::{a.return_type}
            || function.parameters.len() != {a.n_params}
            || !self.frame_slots.is_empty(){gates}
        {{
            return Ok(false);
        }}
        let hash = super::ast_hash(function);
        if hash != {upper}_AST_HASH {{
            eprintln!("{a.name} hash candidate: {{hash:#x}}");
            return Ok(false);
        }}
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {{
            _ => {{
                eprintln!("{a.name} context candidate: {{context:#x}}");
                return Ok(false);
            }}
        }};
        // -- emit (the capture, verbatim) --
{setup}{pool_block}        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in {targets} {{
            labels.insert(target, self.fresh_label());
        }}
{body}
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }}
}}
'''
dest = os.path.join(os.path.dirname(__file__), '..', 'crates/pipeline/mwcc-syntax-trees-to-machine-code/src/captures', a.name + '.rs')
open(dest, 'w').write(out)
print(f"wrote {dest}")
