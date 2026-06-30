// A branchless sign-mask SELECT advances mwcc's anonymous-@N counter by 3 too, like the other
// ternary forms (bool/comparison ternary of 881/882, float branch). `a < 0 ? -1 : 0` lowers to
// `srawi` and `a < 0 ? 0 : -1` to `srwi; addi -1` — the INSTRUCTIONS already matched mwcc; only
// the frame fn's extab/extabindex @N was short by 3 (numbered @5/@6 vs @8/@9). control_flow.rs
// sign_mask_select arm now bumps anonymous_label_bump by 3. A call forces the frame; the value is
// always integer (the srawi/srwi is signed), so no float guard is needed.
extern void g(void);
int smneg(int a) { g(); return a < 0 ? -1 : 0; }
int smcompl(int a) { g(); return a < 0 ? 0 : -1; }
