#define PY_SSIZE_T_CLEAN
#include <Python.h>

/*
 * Keep the CPython layout-dependent Py_REFCNT macro inside the compatibility
 * adapter. Rust and the Tonic runtime only see this narrow function boundary.
 */
Py_ssize_t tonic_cpython_refcount(PyObject *object) {
    return Py_REFCNT(object);
}
