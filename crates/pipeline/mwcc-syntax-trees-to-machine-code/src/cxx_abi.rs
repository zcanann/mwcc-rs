//! C++ ABI products that are not function bodies.

mod adjustor_thunks;

pub(crate) use adjustor_thunks::lower_vtable_adjustor_thunks;
