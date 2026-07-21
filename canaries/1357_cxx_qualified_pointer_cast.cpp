struct Owner
{
    struct Value
    {
        int value;
    };
};

int read_qualified(void* raw)
{
    return ((Owner::Value*)raw)->value;
}
