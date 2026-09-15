#include "tonic.h"

#include <stddef.h>

_Static_assert(sizeof(TonicHandle) == 8, "TonicHandle must remain 64-bit");
_Static_assert(sizeof(TonicPersistentHandle) == 8,
               "TonicPersistentHandle must remain 64-bit");
_Static_assert(offsetof(TonicApi, struct_size) == 0, "size must lead the table");
_Static_assert(offsetof(TonicApi, abi_version) == 4, "version header changed");
_Static_assert(offsetof(TonicApi, capabilities) == 8, "capability header changed");
_Static_assert(sizeof(TonicBuffer) == 72, "64-bit buffer descriptor changed");
_Static_assert(sizeof(TonicTraceVisitor) == 40,
               "64-bit trace visitor descriptor changed");
_Static_assert(sizeof(TonicForeignVTable) == 40,
               "64-bit foreign vtable changed");

static TonicStatus trace_payload(void *payload, TonicTraceVisitor *visitor) {
    return visitor->visit(visitor, *(TonicHandle *)payload);
}

static void destroy_payload(void *payload) { (void)payload; }

static TonicStatus passthrough(TonicContext *context,
                               const TonicHandle *arguments,
                               size_t argument_count,
                               TonicHandle *output) {
    (void)context;
    if (argument_count != 1 || arguments == NULL || output == NULL) {
        return TONIC_STATUS_INVALID_ARGUMENT;
    }
    *output = arguments[0];
    return TONIC_STATUS_OK;
}

static TonicStatus initialize(const TonicApi *api, TonicContext *context) {
    uint32_t version = 0;
    return api->query_capability(context, TONIC_CAPABILITY_CORE, 1, &version);
}

int main(void) {
    const TonicApi *api = NULL;
    TonicNativeFn native = passthrough;
    TonicExtensionInitFn init = initialize;
    TonicForeignVTable foreign = {
        sizeof(TonicForeignVTable), TONIC_ABI_VERSION, 7, TONIC_FOREIGN_OWNED,
        trace_payload, destroy_payload};
    (void)native;
    (void)init;
    (void)foreign;
    return tonic_get_api(TONIC_ABI_VERSION, (uint32_t)sizeof(TonicApi),
                         TONIC_CAP_CORE | TONIC_CAP_CONTAINER_ACCESS_V1 |
                             TONIC_CAP_PROTOCOL_ACCESS_V1 |
                             TONIC_CAP_CROSS_COLLECTOR_V1,
                         &api) == TONIC_STATUS_OK
               ? 0
               : 1;
}
