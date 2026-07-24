// builds: 1.3 1.3.2 2.0 2.0p1 2.6 2.7

void sink(void);

void run_once(void)
{
    do {
        sink();
    } while (0);
}
