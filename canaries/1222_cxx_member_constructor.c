#pragma cplusplus on

class Counter {
public:
    Counter(int, short);
    int value;
    short step;
};

Counter::Counter(int initial, short increment)
    : step(increment)
    , value(initial)
{
}

#pragma cplusplus reset
