// A call THROUGH a function-pointer local aliasing a global (the signal.c dispatch
// tail). Unguarded, the alias folds into the direct call -- `F t = gf; t();` is
// exactly `gf();` (`lwz r12,gf; mtctr; bctrl`). Guarded, the pointer loads into r12,
// the test rides on it, and the guard branches to the shared epilogue:
// `stwu; mflr; stw r0; lwz r12,gf; cmplwi r12,0; beq EPILOGUE; mtctr; bctrl;
// EPILOGUE:`. (A call through a never-allocated fn-ptr local previously emitted a
// direct `bl <local>` -- a relocation against the local's NAME, a miscompile -- because
// the liveness walkers did not count a call's NAME as a read of the variable.)
typedef void (*handler)(void);
handler gf;

void call_through(void)        { handler t = gf; t(); }
void guarded_call_through(void){ handler t = gf; if (!t) return; t(); }
