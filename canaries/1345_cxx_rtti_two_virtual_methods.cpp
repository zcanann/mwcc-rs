class A {
public:
    virtual int first();
    virtual int second();
};

int A::first() {
    return 1;
}

int A::second() {
    return 2;
}
