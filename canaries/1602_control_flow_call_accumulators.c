// flags: -Cpp_exceptions off -pragma "cats off"
// Error reductions must retain their value across zero iterations, backedges,
// and joins. Calls may clobber every volatile register.
int operation(int);

int loop_accumulate(int count) {
    int error = 0;
    error |= !operation(-1);
    while (count > 0) {
        error |= !operation(count);
        --count;
    }
    return !error;
}

int loop_replace(int count) {
    int error = 0;
    error |= !operation(-1);
    while (count > 0) {
        error = !operation(count);
        --count;
    }
    return !error;
}

int conditional_accumulate(int mode) {
    int error = 0;
    error |= !operation(-1);
    if (mode) {
        error |= !operation(1);
    }
    return !error;
}

int alternate_accumulate(int mode) {
    int error = 0;
    error |= !operation(-1);
    if (mode) {
        error |= !operation(1);
    } else {
        error |= !operation(2);
    }
    return !error;
}

int early_return_accumulate(int mode) {
    int error = 0;
    error |= !operation(-1);
    if (mode) {
        error |= !operation(1);
        return !error;
    }
    error |= !operation(2);
    return !error;
}

int continued_accumulate(int count) {
    int error = 0;
    error |= !operation(-1);
    while (count > 0) {
        --count;
        if (count & 1) continue;
        error |= !operation(count);
        if (count == 2) break;
    }
    return !error;
}

int nested_accumulate(int count) {
    int error = 0;
    error |= !operation(-1);
    while (count > 0) {
        if (count & 1) {
            error |= !operation(count);
        } else {
            error |= !operation(-count);
        }
        --count;
    }
    return !error;
}

int goto_accumulate(int count) {
    int error = !operation(-1);
    goto test;
again:
    error |= !operation(count);
    --count;
test:
    if (count > 0) goto again;
    return !error;
}
