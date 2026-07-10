// `.ctors` + `.dtors` section-attributed function-pointer constants, including a pointer to an
// UNDEFINED (extern) function — the full Runtime __init_cpp_exceptions.c data shape. Measured object
// layout (melee ref.o): sections .text, .ctors, .dtors, ...; the chain objects' symbols emit in
// FORWARD declaration order interleaved at their source position, and a relocation target that is
// neither a defined global nor a function defined in this unit gets an UNDEF symbol emitted
// IMMEDIATELY AFTER the referencing object's symbol (`__destroy_global_chain_reference` [.dtors],
// then UNDEF `__destroy_global_chain`, then `__fini_cpp_exceptions_reference` [.dtors]). The
// `.rela.ctors`/`.rela.dtors` sections carry one ADDR32 per entry. Previously the section-attributed
// path skipped the initialized-run symbol pass, so the UNDEF target panicked the writer ("no entry
// found for key") — now `.ctors`/`.dtors` objects join the initialized run. (fire 639 — the flip that
// took melee Runtime/__init_cpp_exceptions.c DEFER->BYTE, the first whole real file.)
extern void __destroy_global_chain(void);
void my_init(void) {}
void my_fini(void) {}
__declspec(section ".ctors") void* const my_init_reference = my_init;
__declspec(section ".dtors") void* const destroy_chain_reference = __destroy_global_chain;
__declspec(section ".dtors") void* const my_fini_reference = my_fini;
