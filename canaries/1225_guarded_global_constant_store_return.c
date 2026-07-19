// builds: 1.3 1.3.2 2.0 2.0p1 2.6 2.7

static int initialized;

int open_once(void)
{
    if (initialized) {
        return -10005;
    }
    initialized = 1;
    return 0;
}
