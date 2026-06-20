// A parameter live across a call goes into the callee-saved register r31 (saved
// in the prologue, reloaded in the epilogue); the unwind table records one saved
// GPR. This is the first callee-saved-register spill.
int g(void);
int calleesave(int a){ g(); return a; }
