// builds: 1.2.5n 1.3 1.3.2 2.0 2.0p1 2.5 2.6 2.7
// flags: -Cpp_exceptions off -O4,p -inline auto

struct ForwardObject;

extern int external_object_test(ForwardObject* object);

int forward_object_test(ForwardObject* object)
{
    return external_object_test(object);
}
