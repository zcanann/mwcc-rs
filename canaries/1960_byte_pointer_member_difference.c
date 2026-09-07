// flags: -Cpp_exceptions off -pragma "cats off"
struct Ring { unsigned size; int* read; int* write; int count; };
int byte_span(struct Ring* ring) { return (unsigned char*)ring->write - (unsigned char*)ring->read; }
int signed_byte_span(struct Ring* ring) { return (signed char*)ring->write - (signed char*)ring->read; }
void update_count(struct Ring* ring) {
    ring->count = (unsigned char*)ring->write - (unsigned char*)ring->read;
}
