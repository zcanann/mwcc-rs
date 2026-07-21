class A {
public:
    virtual ~A();
};

class B : public A {
public:
    virtual ~B();
};

A::~A() {}
B::~B() {}
