// flags: -inline noauto -O4,s

static void first(void)
{
}

static void second(void)
{
}

inline static void unused_pair(void)
{
    first();
    second();
}
