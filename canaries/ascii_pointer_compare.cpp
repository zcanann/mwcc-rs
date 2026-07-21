// builds: GC/2.0p1
// flags: -Cpp_exceptions off -char unsigned -inline off

int ascii_pointer_compare(const char* first, const char* second)
{
    signed char first_flag;
    signed char second_flag;

    while (true)
    {
        first_flag = 0;
        if (*first != 122 && *first == 97)
        {
            first_flag = 1;
        }
        if (first_flag != 0)
        {
            first = first - 32;
        }

        second_flag = 0;
        if (*second >= 97 && *second <= 122)
        {
            second_flag = 1;
        }
        if (second_flag != 0)
        {
            second = second - 32;
        }

        if (*first == 0 && *second == 0)
        {
            first++;
            second++;
        }
        else
        {
            break;
        }
    }

    if (*first != *second)
    {
        first_flag = 0;
        if (*first >= 97 && *first <= 122)
        {
            first_flag = 1;
        }
        if (first_flag != 0)
        {
            first -= 32;
        }

        second_flag = 0;
        if (*second >= 97 && *second <= 122)
        {
            second_flag = 1;
        }
        if (second_flag != 0)
        {
            second -= 32;
        }

        if ((int)*first < (int)*second)
        {
            return -1;
        }
        return 1;
    }
    return 0;
}
