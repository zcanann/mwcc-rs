// builds: 1.3 1.3.2 2.0 2.0p1 2.6 2.7

void consume(unsigned int, unsigned int, void*);

void forward_first_parameter(void* value)
{
    consume(128, 32, value);
}

void forward_second_parameter(void* owner, void* value)
{
    consume(128, 32, value);
}
