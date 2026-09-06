// flags: -sym on -Cpp_exceptions off
/* Names survive declarators whose callable types are not fully modeled yet. */
typedef float Matrix[3][4];
void matrix_ranges(const Matrix lhs, const Matrix* sources, Matrix* outputs, unsigned count);
void (*exchange_handler(void (*next)(unsigned short)))(unsigned short);
__declspec(section ".init") asm void cache_flush(void* address, unsigned length);
void unnamed_controls(const Matrix*, void (*)(unsigned short), unsigned);
int declaration_boundary(void) {
    return 7;
}
