class Base {
public:
    virtual ~Base();
};

class Derived : public Base {
public:
    ~Derived();
};

Base::~Base() {}
Derived::~Derived() {}
