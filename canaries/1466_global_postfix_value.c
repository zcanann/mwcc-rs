// builds: GC/1.2.5 GC/1.3 GC/1.3.2 GC/2.0 GC/2.0p1 GC/2.6 GC/2.7

int counter;

int direct_postfix(void)
{
    return counter++;
}

int before(void);
void after(int);

int saved_postfix(void)
{
    int enabled;
    int previous;
    enabled = before();
    previous = counter++;
    after(enabled);
    return previous;
}
