#pragma cplusplus on

namespace sample {

class Counter {
public:
    void set(int);
    int value;
};

void Counter::set(int input)
{
    value = input;
}

}

#pragma cplusplus reset
