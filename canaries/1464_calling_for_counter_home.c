// builds: 1.3 1.3.2 2.0 2.0p1 2.6 2.7

void consume(int);

void count_calls(void)
{
    int index;
    for (index = 0; index < 4; index = index + 1) {
        consume(index);
    }
}
