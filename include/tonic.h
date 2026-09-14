#ifndef TONIC_H
#define TONIC_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define TONIC_ABI_VERSION 1u

typedef uint64_t TonicHandle;
typedef uint64_t TonicPersistentHandle;
typedef struct TonicContext TonicContext;
typedef struct TonicRuntimeOwner TonicRuntimeOwner;
typedef uint32_t TonicStatus;
typedef uint32_t TonicExceptionKind;
typedef uint32_t TonicCapability;
typedef uint32_t TonicValueKind;
typedef uint32_t TonicDType;
typedef struct TonicTraceVisitor TonicTraceVisitor;

enum {
    TONIC_STATUS_OK = 0,
    TONIC_STATUS_EXCEPTION = 1,
    TONIC_STATUS_INVALID_ARGUMENT = 2,
    TONIC_STATUS_ABI_MISMATCH = 3,
    TONIC_STATUS_UNSUPPORTED = 4,
    TONIC_STATUS_PANIC = 5
};

enum {
    TONIC_VALUE_NONE = 1,
    TONIC_VALUE_BOOL = 2,
    TONIC_VALUE_INT = 3,
    TONIC_VALUE_FLOAT = 4,
    TONIC_VALUE_STR = 5,
    TONIC_VALUE_FOREIGN = 6,
    TONIC_VALUE_LIST = 7,
    TONIC_VALUE_TUPLE = 8,
    TONIC_VALUE_DICT = 9,
    TONIC_VALUE_OTHER = 255
};

enum {
    TONIC_EXCEPTION_RUNTIME_ERROR = 1,
    TONIC_EXCEPTION_TYPE_ERROR = 2,
    TONIC_EXCEPTION_VALUE_ERROR = 3,
    TONIC_EXCEPTION_OVERFLOW_ERROR = 4,
    TONIC_EXCEPTION_HANDLE_ERROR = 5,
    TONIC_EXCEPTION_PYTHON_ERROR = 6
};

enum {
    TONIC_CAPABILITY_CORE = 1,
    TONIC_CAPABILITY_EXPLICIT_EXCEPTION_STATUS = 2,
    TONIC_CAPABILITY_SCOPED_LOCAL_HANDLES = 3,
    TONIC_CAPABILITY_PANIC_GUARD = 4,
    TONIC_CAPABILITY_BUFFER_V1 = 5,
    TONIC_CAPABILITY_FOREIGN_OBJECT_V1 = 6,
    TONIC_CAPABILITY_PERSISTENT_HANDLES_V1 = 7,
    TONIC_CAPABILITY_RUNTIME_OWNER_V1 = 8,
    TONIC_CAPABILITY_CONTAINER_ACCESS_V1 = 9,
    TONIC_CAPABILITY_PROTOCOL_ACCESS_V1 = 10
};

#define TONIC_CAP_CORE (UINT64_C(1) << 0)
#define TONIC_CAP_EXPLICIT_EXCEPTION_STATUS (UINT64_C(1) << 1)
#define TONIC_CAP_SCOPED_LOCAL_HANDLES (UINT64_C(1) << 2)
#define TONIC_CAP_PANIC_GUARD (UINT64_C(1) << 3)
#define TONIC_CAP_BUFFER_V1 (UINT64_C(1) << 4)
#define TONIC_CAP_FOREIGN_OBJECT_V1 (UINT64_C(1) << 5)
#define TONIC_CAP_PERSISTENT_HANDLES_V1 (UINT64_C(1) << 6)
#define TONIC_CAP_RUNTIME_OWNER_V1 (UINT64_C(1) << 7)
#define TONIC_CAP_CONTAINER_ACCESS_V1 (UINT64_C(1) << 8)
#define TONIC_CAP_PROTOCOL_ACCESS_V1 (UINT64_C(1) << 9)

#define TONIC_FOREIGN_OWNED (UINT64_C(1) << 0)

typedef TonicStatus (*TonicForeignTraceFn)(void *, TonicTraceVisitor *);
typedef void (*TonicForeignDestroyFn)(void *);

struct TonicTraceVisitor {
    uint32_t struct_size;
    uint32_t reserved;
    void *state;
    TonicStatus (*visit)(TonicTraceVisitor *, TonicHandle);
};

typedef struct TonicForeignVTable {
    uint32_t struct_size;
    uint32_t abi_version;
    uint64_t adapter_id;
    uint64_t flags;
    TonicForeignTraceFn trace;
    TonicForeignDestroyFn destroy;
} TonicForeignVTable;

enum {
    TONIC_DTYPE_I8 = 1,
    TONIC_DTYPE_U8 = 2,
    TONIC_DTYPE_I16 = 3,
    TONIC_DTYPE_U16 = 4,
    TONIC_DTYPE_I32 = 5,
    TONIC_DTYPE_U32 = 6,
    TONIC_DTYPE_I64 = 7,
    TONIC_DTYPE_U64 = 8,
    TONIC_DTYPE_F16 = 9,
    TONIC_DTYPE_F32 = 10,
    TONIC_DTYPE_F64 = 11,
    TONIC_DTYPE_BOOL = 12,
    TONIC_DTYPE_COMPLEX64 = 13,
    TONIC_DTYPE_COMPLEX128 = 14
};

#define TONIC_BUFFER_WRITABLE (UINT64_C(1) << 0)
#define TONIC_BUFFER_C_CONTIGUOUS (UINT64_C(1) << 1)

typedef struct TonicBuffer {
    uint32_t struct_size;
    TonicDType dtype;
    uint64_t flags;
    uint8_t *data;
    size_t byte_len;
    size_t item_size;
    uint32_t ndim;
    uint32_t reserved;
    const size_t *shape;
    const ptrdiff_t *strides;
    TonicHandle owner;
} TonicBuffer;

typedef struct TonicApi {
    uint32_t struct_size;
    uint32_t abi_version;
    uint64_t capabilities;

    TonicStatus (*status_clear)(TonicContext *);
    TonicStatus (*exception_kind)(TonicContext *, uint8_t *, size_t, size_t *);
    TonicStatus (*exception_message)(TonicContext *, uint8_t *, size_t, size_t *);
    TonicStatus (*raise_exception)(TonicContext *, TonicExceptionKind,
                                   const uint8_t *, size_t);
    TonicStatus (*query_capability)(TonicContext *, TonicCapability, uint32_t,
                                    uint32_t *);
    TonicStatus (*none)(TonicContext *, TonicHandle *);
    TonicStatus (*int_from_i64)(TonicContext *, int64_t, TonicHandle *);
    TonicStatus (*int_as_i64)(TonicContext *, TonicHandle, int64_t *);
    TonicStatus (*float_from_f64)(TonicContext *, double, TonicHandle *);
    TonicStatus (*float_as_f64)(TonicContext *, TonicHandle, double *);
    TonicStatus (*str_from_utf8)(TonicContext *, const uint8_t *, size_t,
                                 TonicHandle *);
    TonicStatus (*str_utf8)(TonicContext *, TonicHandle, uint8_t *, size_t,
                            size_t *);
    TonicStatus (*add)(TonicContext *, TonicHandle, TonicHandle, TonicHandle *);
    TonicStatus (*call)(TonicContext *, TonicHandle, const TonicHandle *, size_t,
                        TonicHandle *);
    TonicStatus (*buffer_export)(TonicContext *, TonicHandle, uint64_t,
                                 TonicBuffer *);
    TonicStatus (*buffer_release)(TonicContext *, TonicBuffer *);
    TonicStatus (*foreign_reference_create)(TonicContext *, TonicHandle,
                                             TonicHandle *);
    TonicStatus (*foreign_reference_release)(TonicContext *, TonicHandle);
    TonicStatus (*foreign_create)(TonicContext *, void *,
                                  const TonicForeignVTable *, TonicHandle *);
    TonicStatus (*foreign_borrow_payload)(TonicContext *, TonicHandle, uint64_t,
                                          void **);
    TonicStatus (*persistent_create)(TonicContext *, TonicHandle,
                                     TonicPersistentHandle *);
    TonicStatus (*persistent_borrow)(TonicContext *, TonicPersistentHandle,
                                     TonicHandle *);
    TonicStatus (*persistent_release)(TonicContext *, TonicPersistentHandle);
    TonicStatus (*runtime_owner_acquire)(TonicContext *, TonicRuntimeOwner **);
    TonicStatus (*runtime_owner_release)(TonicRuntimeOwner *);
    TonicStatus (*runtime_owner_matches)(TonicContext *, const TonicRuntimeOwner *,
                                         uint32_t *);
    TonicStatus (*runtime_execution_id)(TonicContext *, uint64_t *);
    TonicStatus (*persistent_release_deferred)(const TonicRuntimeOwner *,
                                               TonicPersistentHandle);
    TonicStatus (*value_kind)(TonicContext *, TonicHandle, TonicValueKind *);
    TonicStatus (*is_identical)(TonicContext *, TonicHandle, TonicHandle,
                                uint32_t *);
    TonicStatus (*bool_from)(TonicContext *, uint32_t, TonicHandle *);
    TonicStatus (*bool_as)(TonicContext *, TonicHandle, uint32_t *);
    TonicStatus (*int_from_decimal)(TonicContext *, const uint8_t *, size_t,
                                    TonicHandle *);
    TonicStatus (*int_decimal)(TonicContext *, TonicHandle, uint8_t *, size_t,
                               size_t *);
    TonicStatus (*list_new)(TonicContext *, TonicHandle *);
    TonicStatus (*list_append)(TonicContext *, TonicHandle, TonicHandle);
    TonicStatus (*tuple_new)(TonicContext *, const TonicHandle *, size_t,
                             TonicHandle *);
    TonicStatus (*sequence_len)(TonicContext *, TonicHandle, size_t *);
    TonicStatus (*sequence_get)(TonicContext *, TonicHandle, size_t,
                                TonicHandle *);
    TonicStatus (*dict_new)(TonicContext *, TonicHandle *);
    TonicStatus (*dict_len)(TonicContext *, TonicHandle, size_t *);
    TonicStatus (*dict_entry)(TonicContext *, TonicHandle, size_t,
                              TonicHandle *, TonicHandle *);
    TonicStatus (*dict_set)(TonicContext *, TonicHandle, TonicHandle,
                            TonicHandle);
    TonicStatus (*call_kw)(TonicContext *, TonicHandle, const TonicHandle *,
                           size_t, TonicHandle, TonicHandle *);
    TonicStatus (*get_attr)(TonicContext *, TonicHandle, const uint8_t *,
                            size_t, TonicHandle *);
    TonicStatus (*set_attr)(TonicContext *, TonicHandle, const uint8_t *,
                            size_t, TonicHandle);
    TonicStatus (*repr_value)(TonicContext *, TonicHandle, TonicHandle *);
    TonicStatus (*foreign_reference_borrow)(TonicContext *, TonicHandle,
                                             TonicHandle *);
} TonicApi;

typedef TonicStatus (*TonicNativeFn)(TonicContext *, const TonicHandle *,
                                     size_t, TonicHandle *);
typedef TonicStatus (*TonicExtensionInitFn)(const TonicApi *, TonicContext *);

TonicStatus tonic_get_api(uint32_t abi_version, uint32_t minimum_struct_size,
                          uint64_t required_capabilities,
                          const TonicApi **output);

#ifdef __cplusplus
}
#endif

#endif
