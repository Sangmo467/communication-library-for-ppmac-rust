#include "pmac.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <inttypes.h>

static pmac_bytes bytes(const char *s) {
    pmac_bytes b = {(const uint8_t *)s, s ? strlen(s) : 0};
    return b;
}
static int32_t on_message(void *user, int32_t status, uint32_t kind,
                          const uint8_t *text, size_t length) {
    FILE *output = status == PMAC_OK ? stdout : stderr;
    (void)user;
    fprintf(output, "[kind=%u status=%d] ", (unsigned)kind, (int)status);
    if (length) fwrite(text, 1, length, output); /* length-delimited, not NUL-terminated */
    fputc('\n', output);
    return 0; /* nonzero requests stop; never call stop/release from this callback */
}
int main(void) {
    pmac_runtime_id runtime = 0;
    pmac_client_id client = 0;
    pmac_request_id request = 0;
    pmac_geterrors_id monitor = 0;
    pmac_runtime_config rc = {0};
    pmac_client_config cc = {0};
    pmac_connect_options connection = {0};
    pmac_geterrors_config config = {0};
    pmac_request_info info = {0};
    const char *host = getenv("PMAC_HOST");
    const char *known_hosts = getenv("PMAC_KNOWN_HOSTS");
    int32_t status = PMAC_OK, stopped, released;
    int failed = 0;
    if (!host || !*host || !known_hosts || !*known_hosts) {
        fputs("Set PMAC_HOST and PMAC_KNOWN_HOSTS\n", stderr);
        return 1;
    }
    rc.struct_size = sizeof(rc); rc.abi_version = PMAC_ABI_VERSION; rc.worker_threads = 2;
    status = pmac_runtime_create(&rc, &runtime);
    if (status != PMAC_OK) goto cleanup;
    cc.struct_size = sizeof(cc); cc.abi_version = PMAC_ABI_VERSION;
    cc.max_requests = 64; cc.max_file_tasks = 1; cc.max_retained_bytes = 80u * 1024u * 1024u;
    status = pmac_client_create(runtime, &cc, &client);
    if (status != PMAC_OK) goto cleanup;
    connection.struct_size = sizeof(connection); connection.abi_version = PMAC_ABI_VERSION;
    connection.host = bytes(host); connection.username = bytes("root");
    connection.password = bytes(getenv("PMAC_PASSWORD")); connection.known_hosts = bytes(known_hosts);
    connection.port = 22; connection.control_pool_size = 1;
    connection.flags = PMAC_CONNECT_TCP_PROBE; connection.tcp_probe_timeout_ms = 600;
    connection.keepalive_interval_ms = 1000; connection.keepalive_max = 3;
    status = pmac_connect_submit(client, &connection, &request);
    if (status != PMAC_OK) goto cleanup;
    do { status = pmac_request_wait(request, UINT32_MAX, &info); }
    while (status == PMAC_WAIT_PENDING);
    (void)pmac_request_release(request); request = 0;
    if (status != PMAC_OK) goto cleanup;
    config.struct_size = sizeof(config); config.abi_version = PMAC_ABI_VERSION;
    config.period_seconds = 0.01; /* required: zero does NOT select a default */
    config.flags = PMAC_GETERRORS_DATE | PMAC_GETERRORS_GLOBAL_MASK |
                   PMAC_GETERRORS_COORD_MASK | PMAC_GETERRORS_MOTOR_MASK | PMAC_GETERRORS_RING_MASK;
    config.global_mask = UINT64_C(0x7ff); config.coord_mask = UINT64_C(0x180000000807ffc);
    config.motor_mask = UINT64_C(0x8c007ffc); config.ring_mask = UINT64_C(0x803e);
    status = pmac_geterrors_start(client, &config, on_message, NULL, &monitor);
    if (status != PMAC_OK) goto cleanup;
    puts("Monitoring. Press Enter to stop.");
    (void)getchar();
cleanup:
    if (status != PMAC_OK) { fprintf(stderr, "operation failed: %d\n", (int)status); failed = 1; }
    if (monitor) {
        stopped = pmac_geterrors_stop(monitor);
        released = pmac_geterrors_release(monitor); monitor = 0;
        /* A recorded terminal error can be returned by release after ID destruction. */
        if (stopped != PMAC_OK || released != PMAC_OK) {
            fprintf(stderr, "Monitor stop=%d release=%d\n", (int)stopped, (int)released);
            failed = 1;
        }
    }
    if (client) {
        status = pmac_client_close(client, 0, 5000);
        if (status == PMAC_OK) status = pmac_client_release(client);
        if (status != PMAC_OK) {
            fprintf(stderr, "Client cleanup failed/pending: %d; client=%" PRIu64
                    " runtime=%" PRIu64 " retained\n", (int)status, client, runtime);
            return 1; /* standalone process exits; embedding applications retain and retry IDs */
        }
    }
    if (runtime) {
        status = pmac_runtime_shutdown(runtime, 5000);
        if (status == PMAC_OK) status = pmac_runtime_release(runtime);
        if (status != PMAC_OK) {
            fprintf(stderr, "Runtime cleanup failed/pending: %d; id=%" PRIu64
                    " retained\n", (int)status, runtime);
            return 1;
        }
    }
    return failed;
}
