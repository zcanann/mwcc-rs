class A {
public:
    virtual int value();
    virtual ~A();
};

int A::value() {
    return 1;
}

A::~A() {}
