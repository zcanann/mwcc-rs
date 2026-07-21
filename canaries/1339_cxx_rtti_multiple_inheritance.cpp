class A {
public:
    virtual ~A();
};

class B {
public:
    virtual ~B();
};

class C : public A, public B {
public:
    virtual ~C();
};

class D {
public:
    virtual ~D();
};

class E : public C, public D {
public:
    virtual ~E();
};

A::~A() {}
B::~B() {}
C::~C() {}
D::~D() {}
E::~E() {}
