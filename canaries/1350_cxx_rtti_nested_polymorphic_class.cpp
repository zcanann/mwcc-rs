class Outer {
public:
    class Nested {
    public:
        virtual int value();
    };
};

int Outer::Nested::value() {
    return 1;
}
