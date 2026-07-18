// A frameless leaf before a plain direct caller probes both the translation
// unit's anonymous-number base/leaf stride and the generation's linkage frame.
extern void callee(void);

void empty_leaf(unsigned unused)
{
}

void plain_nonleaf(void)
{
    callee();
}
