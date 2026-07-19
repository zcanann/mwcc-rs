#pragma cplusplus on

extern "C" {
typedef struct {
    char gpr;
    char fpr;
    char reserved[2];
    char* input_arg_area;
    char* reg_save_area;
} __va_list[1];
typedef __va_list va_list;

int linkage_typedef_size(void)
{
    va_list args;
    {}
    char buffer[4];
    return sizeof(args) + sizeof(buffer);
}
}

#pragma cplusplus reset
