int false_when_zero(float value)
{
    if (!value) {
        return 1;
    }
    return 0;
}

int true_when_nonzero(float value)
{
    if (value) {
        return 1;
    }
    return 0;
}
