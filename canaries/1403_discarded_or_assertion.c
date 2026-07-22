void report_failure(const char* file, int line, const char* expression, ...);

void assert_channel(int channel)
{
    (void)((0 <= channel && channel < 2) ||
           (report_failure("card.c", 216, "0 <= channel && channel < 2"), 0));
}
