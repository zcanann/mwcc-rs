class A {
public:
    virtual int value();
};

class Outer {
public:
    class Nested {
    public:
        virtual int value();
    };
};

int A::value() {
    return 1;
}

int Outer::Nested::value() {
    return 2;
}
