/* Fire 411: the FPCLASSIFY SWITCH — statement-bodied arms with
   short-circuit || diamonds over the pun words. hx lives in r4,
   the scrutinee rlwinm to r3, the tree compares against the
   lis-built big value (cmpw) then 0; per arm: clrlwi. record ->
   bne TRUE; lwz the low word from the SPILL; cmpwi; beq FALSE;
   li/b-END per side. The default may be the trailing return
   after the switch. @N +13. */
int f(double x)
{
    switch ((*(int *)&x) & 0x7FF00000) {
    case 0x7FF00000: {
        if (((*(int *)&x) & 0xFFFFF) || (*(1 + (int *)&x))) {
            return 3;
        } else {
            return 2;
        }
        break;
    }
    case 0: {
        if (((*(int *)&x) & 0xFFFFF) || (*(1 + (int *)&x))) {
            return 5;
        } else {
            return 4;
        }
        break;
    }
    default:
        return 1;
    }
}
