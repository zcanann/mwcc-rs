// A prior `static` declaration gives the later definition internal linkage
// even when the definition does not repeat the storage-class specifier.
static int helper(void);

int helper(void)
{
    return 1;
}
