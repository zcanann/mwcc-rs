class HeaderOnly {
public:
    int helper() { return 0; }
};

class A {
public:
    virtual int value();
};

int A::value() {
    return 1;
}
