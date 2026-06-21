// A non-leaf prologue's mflr->LR-store latency holds at most two instructions;
// mwcc fills it with the leading run of register-ALU argument setup — parameter
// copies (`mr`) and register `addi`, ready at entry — and leaves the rest (and any
// constant/memory load) after the store. Passing non-first parameters as arguments
// produces exactly that run of `mr`s.
void sink(int);
void sink2(int, int);
void sink3(int, int, int);
void move_one(int a, int b)                 { sink(b); }         /* mr;        store (1) */
void move_two(int a, int b, int c)          { sink2(b, c); }     /* mr mr;     store (2) */
void move_three(int a, int b, int c, int d) { sink3(b, c, d); }  /* mr mr; store; mr (2+1) */
int counter;
void const_first(void)                      { sink(5); }         /* store; li  (not hoisted) */
void global_first(void)                     { sink(counter); }   /* store; lwz (not hoisted) */
