// flags: -Cpp_exceptions off -pragma "cats off"
typedef struct Region { unsigned value; } Region;
typedef struct Object { unsigned prefix; unsigned key; } Object;
typedef struct Hooks { unsigned prefix; Region* (*lookup)(unsigned); } Hooks;
extern Hooks* hooks;
unsigned callback_value(Object* object) {
    Object* alias = (Object*)object;
    Region* region;
    region = (Region*)hooks->lookup(alias->key);
    return region->value + object->prefix;
}
unsigned member_callback(Hooks* table, Object* object) {
    Region* region;
    region = table->lookup(object->key);
    return region->value;
}
