// The setjmp/longjmp inline-asm vocabulary (Gecko_setjmp.c — flipped whole-file BYTE by this):
// - REGISTER PARAMETERS named in operands: `env` (r3) and `val` (r4) resolve positionally, both as
//   plain register operands (`mr r3,val`, `cmpwi val,0`) and as member bases.
// - MEMBER operands `env->field`: a displacement memory operand off the parameter's register, the
//   offset from the struct layout (`stw r5,env->pc` -> `stw r5,0(r3)`; an ARRAY field like
//   `env->grps` yields its offset: `stmw r13,env->gprs` -> `stmw r13,20(r3)`).
// - New mnemonics, measured encodings: mflr/mtlr (mfspr/mtspr LR), mfcr, mffs (fc 00 04 8e),
//   mtcrf 255,rS (7c cf f1 20), mtfsf 255,frB (fd fe 05 8e), stmw (opcode 47), lmw (opcode 46).
// (fire 640 — the second real-file flip)
typedef struct my_buf {
    unsigned long pc;
    unsigned long cr;
    unsigned long sp;
    unsigned long rtoc;
    unsigned long reserved;
    unsigned long gprs[19];
    double fp31;
    double fpscr;
} my_buf;

asm int my_setjmp(register my_buf* env)
{
    nofralloc
    mflr    r5
    mfcr    r6
    stw     r5,env->pc
    stw     r6,env->cr
    stw     SP,env->sp
    stw     RTOC,env->rtoc
    stmw    r13,env->gprs
    mffs    fp0
    stfd    fp31,env->fp31
    stfd    fp0,env->fpscr
    li      r3,0
    blr
}

asm void my_longjmp(register my_buf* env, register int val)
{
    nofralloc
    lwz     r5,env->pc
    lwz     r6,env->cr
    mtlr    r5
    mtcrf   255,r6
    lwz     SP,env->sp
    lwz     RTOC,env->rtoc
    lmw     r13,env->gprs
    lfd     fp0,env->fpscr
    lfd     fp31,env->fp31
    cmpwi   val,0
    mr      r3,val
    mtfsf   255,fp0
    bnelr
    li      r3,1
    blr
}
