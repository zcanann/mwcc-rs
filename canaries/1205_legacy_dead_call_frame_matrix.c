// Dead call-initializer locals preserve their calls for side effects. Contrast
// them with directly discarded calls to characterize build 163's frame sizing.
int dead_frame_call(void);

int direct_one_one_param(int a)
{
    dead_frame_call();
    return a;
}

int dead_one_one_param(int a)
{
    int unused = dead_frame_call();
    return a;
}

int direct_two_one_param(int a)
{
    dead_frame_call();
    dead_frame_call();
    return a;
}

int dead_two_one_param(int a)
{
    int unused0 = dead_frame_call();
    int unused1 = dead_frame_call();
    return a;
}

int direct_one_two_params(int a, int b)
{
    dead_frame_call();
    return a + b;
}

int dead_one_two_params(int a, int b)
{
    int unused = dead_frame_call();
    return a + b;
}
