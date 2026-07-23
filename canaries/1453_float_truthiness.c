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

int true_when_not_less(float left, float right)
{
    if (!(left < right)) {
        return 1;
    }
    return 0;
}
