class Base {
public:
    virtual int value();
};

class Derived : public Base {
public:
    int value();
};

int Base::value() {
    return 1;
}

int Derived::value() {
    return 2;
}
