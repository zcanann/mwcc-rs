struct Outer
{
    struct Inner
    {
        Inner* next;
        Inner* keep();
    };
};

Outer::Inner* Outer::Inner::keep()
{
    Inner* current = this;
    return current;
}
