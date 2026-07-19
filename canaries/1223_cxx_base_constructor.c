#pragma cplusplus on

class BaseCounter {
public:
    BaseCounter(int);
    int base_value;
};

BaseCounter::BaseCounter(int initial)
    : base_value(initial)
{
}

class DerivedCounter : public BaseCounter {
public:
    DerivedCounter(int, short);
    short derived_value;
};

DerivedCounter::DerivedCounter(int base, short derived)
    : BaseCounter(base)
    , derived_value(derived)
{
}

#pragma cplusplus reset
