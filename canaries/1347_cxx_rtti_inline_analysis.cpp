class A {
public:
    int helper() { return 0; }
    virtual int value();
};

int A::value() {
    return helper();
}
