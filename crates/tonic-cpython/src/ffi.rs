use std::ffi::{c_char, c_double, c_int, c_longlong, c_void};

#[repr(C)]
pub struct PyObject {
    _private: [u8; 0],
}
#[repr(C)]
pub struct PyThreadState {
    _private: [u8; 0],
}

pub type PySsizeT = isize;
pub type PyGilState = c_int;
#[repr(C)]
pub struct PyTypeSlot {
    pub slot: c_int,
    pub function: *mut c_void,
}

#[repr(C)]
pub struct PyTypeSpec {
    pub name: *const c_char,
    pub basic_size: c_int,
    pub item_size: c_int,
    pub flags: u32,
    pub slots: *mut PyTypeSlot,
}

pub const PY_TP_CALL: c_int = 50;
pub const PY_TP_DEALLOC: c_int = 52;
pub const PY_TP_GETATTRO: c_int = 58;
pub const PY_TP_REPR: c_int = 66;
pub const PY_TP_SETATTRO: c_int = 69;
pub const PY_TP_FREE: c_int = 74;

extern "C" {
    pub fn Py_IsInitialized() -> c_int;
    pub fn Py_Initialize();
    pub fn PyEval_SaveThread() -> *mut PyThreadState;
    pub fn PyGILState_Ensure() -> PyGilState;
    pub fn PyGILState_Release(state: PyGilState);
    pub fn Py_DecRef(object: *mut PyObject);
    pub fn Py_IncRef(object: *mut PyObject);
    pub fn Py_REFCNT(object: *mut PyObject) -> PySsizeT;
    pub fn PyErr_Occurred() -> *mut PyObject;
    pub fn PyErr_Clear();
    pub fn PyErr_SetString(exception: *mut PyObject, message: *const c_char);
    pub static mut PyExc_RuntimeError: *mut PyObject;
    pub static mut PyExc_AttributeError: *mut PyObject;
    pub static mut PyExc_TypeError: *mut PyObject;
    pub static mut PyExc_ValueError: *mut PyObject;
    pub static mut PyExc_OverflowError: *mut PyObject;
    pub static mut _Py_NoneStruct: PyObject;
    pub static mut _Py_TrueStruct: PyObject;
    pub static mut _Py_FalseStruct: PyObject;
    pub static mut PyLong_Type: PyObject;
    pub static mut PyFloat_Type: PyObject;
    pub static mut PyUnicode_Type: PyObject;
    pub static mut PyList_Type: PyObject;
    pub static mut PyTuple_Type: PyObject;
    pub static mut PyDict_Type: PyObject;
    pub fn PyErr_GetRaisedException() -> *mut PyObject;
    pub fn PyObject_Str(object: *mut PyObject) -> *mut PyObject;
    pub fn PyUnicode_Join(separator: *mut PyObject, sequence: *mut PyObject) -> *mut PyObject;

    pub fn PyLong_FromLongLong(value: c_longlong) -> *mut PyObject;
    pub fn PyLong_FromString(
        value: *const c_char,
        end: *mut *mut c_char,
        base: c_int,
    ) -> *mut PyObject;
    pub fn PyBool_FromLong(value: std::ffi::c_long) -> *mut PyObject;
    pub fn PyLong_AsLongLongAndOverflow(object: *mut PyObject, overflow: *mut c_int) -> c_longlong;
    pub fn PyFloat_FromDouble(value: c_double) -> *mut PyObject;
    pub fn PyFloat_AsDouble(object: *mut PyObject) -> c_double;
    pub fn PyUnicode_FromStringAndSize(value: *const c_char, size: PySsizeT) -> *mut PyObject;
    pub fn PyUnicode_AsUTF8AndSize(object: *mut PyObject, size: *mut PySsizeT) -> *const c_char;
    pub fn PyNumber_Absolute(object: *mut PyObject) -> *mut PyObject;
    pub fn PyList_New(size: PySsizeT) -> *mut PyObject;
    pub fn PyList_Append(list: *mut PyObject, item: *mut PyObject) -> c_int;
    pub fn PyList_Size(list: *mut PyObject) -> PySsizeT;
    pub fn PyList_GetItem(list: *mut PyObject, index: PySsizeT) -> *mut PyObject;
    pub fn PyList_SetItem(list: *mut PyObject, index: PySsizeT, item: *mut PyObject) -> c_int;
    pub fn PyTuple_New(size: PySsizeT) -> *mut PyObject;
    pub fn PyTuple_Size(tuple: *mut PyObject) -> PySsizeT;
    pub fn PyTuple_GetItem(tuple: *mut PyObject, index: PySsizeT) -> *mut PyObject;
    pub fn PyTuple_SetItem(tuple: *mut PyObject, index: PySsizeT, item: *mut PyObject) -> c_int;
    pub fn PyDict_New() -> *mut PyObject;
    pub fn PyDict_SetItem(dict: *mut PyObject, key: *mut PyObject, value: *mut PyObject) -> c_int;
    pub fn PyDict_Next(
        dict: *mut PyObject,
        position: *mut PySsizeT,
        key: *mut *mut PyObject,
        value: *mut *mut PyObject,
    ) -> c_int;
    pub fn PyObject_Length(object: *mut PyObject) -> PySsizeT;
    pub fn PyObject_IsInstance(object: *mut PyObject, class: *mut PyObject) -> c_int;
    pub fn PyImport_ImportModule(name: *const std::ffi::c_char) -> *mut PyObject;
    pub fn PyObject_GetAttrString(
        object: *mut PyObject,
        name: *const std::ffi::c_char,
    ) -> *mut PyObject;
    pub fn PyObject_CallOneArg(function: *mut PyObject, argument: *mut PyObject) -> *mut PyObject;
    pub fn PyObject_Call(
        callable: *mut PyObject,
        arguments: *mut PyObject,
        keywords: *mut PyObject,
    ) -> *mut PyObject;
    pub fn PySequence_Tuple(sequence: *mut PyObject) -> *mut PyObject;
    pub fn PyType_FromSpec(spec: *mut PyTypeSpec) -> *mut PyObject;
    pub fn PyType_GetSlot(class: *mut PyObject, slot: c_int) -> *mut c_void;
    pub fn PyType_GenericAlloc(class: *mut PyObject, size: PySsizeT) -> *mut PyObject;
    pub fn PyObject_GetTypeData(object: *mut PyObject, class: *mut PyObject) -> *mut c_void;
    pub fn PyObject_Vectorcall(
        callable: *mut PyObject,
        arguments: *const *mut PyObject,
        argument_count: usize,
        keyword_names: *mut PyObject,
    ) -> *mut PyObject;
}
