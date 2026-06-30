// An INDIRECT call (`mtctr r12; bctrl`) followed by returning a CONSTANT: mwcc hoists the epilogue's
// saved-LR reload (`lwz r0,20(r1)`) to immediately after the call, BEFORE the return-value `li`
// (`bctrl; lwz r0; li r3,0; mtlr`). The LR-reload hoist (mwcc-vreg schedule.rs) only recognized a
// direct `bl` (BranchAndLink), so an indirect `bctrl` (BranchToCountRegisterAndLink) call left the
// reload after the `li` — a byte-different epilogue order. The hoist now matches both call forms.
// A direct call + const return already worked; a call whose result IS returned has no extra `li`.
int call_ret0(void (*fp)(void))  { fp(); return 0; }
int call_ret7(void (*fp)(void))  { fp(); return 7; }
