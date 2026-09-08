// flags: -Cpp_exceptions off -pragma "cats off"
typedef struct Result { unsigned pad[2]; unsigned value; } Result;
typedef struct Input { unsigned pad[3]; unsigned key; } Input;
typedef struct Table { unsigned pad[4]; Result* (*lookup)(unsigned); } Table;
unsigned object_last(Table* table, Input* input) { Result* result; result = table->lookup(input->key); return result->value; }
unsigned table_last(Input* input, Table* table) { Result* result; result = table->lookup(input->key); return result->value; }
unsigned initialized(Table* table, Input* input) { Result* result = table->lookup(input->key); return result->value; }
unsigned plus(Table* table, Input* input) { Result* result; result = table->lookup(input->key); return result->value + 5; }
unsigned bypass(Table* table, Input* input) { return ((Result*)table->lookup(input->key))->value; }
