#include "pmac.h"
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static pmac_bytes bytes(const char* value) {
    pmac_bytes result = { (const uint8_t*)value, value ? strlen(value) : 0 };
    return result;
}
/* Copy error details before any unrelated PMAC call or Request release. */
static void report_error(int32_t status, pmac_request_id request) {
    pmac_error_info info = {0};
    size_t required = 0;
    int32_t copied = request ? pmac_request_copy_error(request, &info, NULL, 0, &required)
                             : pmac_copy_last_error(&info, NULL, 0, &required);
    if ((copied == PMAC_OK || copied == PMAC_BUFFER_TOO_SMALL) && info.status == status) {
        char* message = required ? (char*)malloc(required) : NULL;
        if (!required || message) {
            copied = request ? pmac_request_copy_error(request, &info, message, required, &required)
                             : pmac_copy_last_error(&info, message, required, &required);
            if (copied == PMAC_OK) {
                fprintf(stderr, "PMAC status=%d certainty=%u request=%" PRIu64 ": ",
                        (int)status, info.certainty, info.request_id);
                if (required) (void)fwrite(message, 1, required, stderr);
                fputc('\n', stderr);
                free(message);
                return;
            }
            free(message);
        }
    }
    fprintf(stderr, "PMAC failed with status %d; execution certainty unavailable\n", (int)status);
}
static int32_t wait_request(pmac_request_id request) {
    pmac_request_info info = {0};
    int32_t status;
    do { status = pmac_request_wait(request, 1000, &info); } while (status == PMAC_WAIT_PENDING);
    return status;
}
int main(int argc, char** argv) {
    pmac_runtime_id runtime = 0;
    pmac_client_id client = 0;
    pmac_request_id request = 0;
    pmac_runtime_config rc = {0};
    pmac_client_config cc = {0};
    pmac_connect_options connection = {0};
    pmac_request_options options = {0};
    const char* command = argc > 1 ? argv[1] : "Sys.ServoTime";
    const char* pattern = argc > 2 ? argv[2] : "";
    const char* host = getenv("PMAC_HOST");
    const char* known_hosts = getenv("PMAC_KNOWN_HOSTS");
    int32_t status = PMAC_OK;
    size_t required = 0;
    uint8_t* response = NULL;
    int failed = 1;
    if (argc > 3 || (argc > 1 && strcmp(argv[1], "--help") == 0)) {
        puts("Usage: getresponse_c [command [pattern]]\nDefault command: Sys.ServoTime\n"
             "Set PMAC_HOST, PMAC_KNOWN_HOSTS and authentication environment variables.");
        return argc > 3 ? 2 : 0;
    }
    if (!host || !*host || !known_hosts || !*known_hosts) {
        fputs("Set PMAC_HOST and PMAC_KNOWN_HOSTS\n", stderr); return 2;
    }
    rc.struct_size = sizeof(rc); rc.abi_version = PMAC_ABI_VERSION; rc.worker_threads = 2;
    status = pmac_runtime_create(&rc, &runtime);
    if (status != PMAC_OK) goto error;
    cc.struct_size = sizeof(cc); cc.abi_version = PMAC_ABI_VERSION;
    cc.max_requests = 64; cc.max_file_tasks = 1; cc.max_retained_bytes = 80u * 1024u * 1024u;
    status = pmac_client_create(runtime, &cc, &client);
    if (status != PMAC_OK) goto error;
    connection.struct_size = sizeof(connection); connection.abi_version = PMAC_ABI_VERSION;
    connection.host = bytes(host); connection.known_hosts = bytes(known_hosts);
    connection.username = bytes(getenv("PMAC_USER") ? getenv("PMAC_USER") : "root");
    connection.password = bytes(getenv("PMAC_PASSWORD"));
    connection.private_key = bytes(getenv("PMAC_PRIVATE_KEY"));
    connection.passphrase = bytes(getenv("PMAC_PASSPHRASE"));
    connection.port = 22; connection.control_pool_size = 1;
    connection.flags = PMAC_CONNECT_TCP_PROBE; connection.tcp_probe_timeout_ms = 600;
    connection.keepalive_interval_ms = 1000; connection.keepalive_max = 3;
    status = pmac_connect_submit(client, &connection, &request);
    if (status != PMAC_OK) goto error;
    status = wait_request(request);
    if (status != PMAC_OK) { report_error(status, request); goto cleanup; }
    status = pmac_request_release(request); request = 0;
    if (status != PMAC_OK) goto error;
    options.struct_size = sizeof(options); options.abi_version = PMAC_ABI_VERSION;
    options.timeout_ms = 5000; options.max_response_bytes = 1024u * 1024u;
    status = pmac_get_response_async(client, (const uint8_t*)command, strlen(command), &options, &request);
    if (status != PMAC_OK) goto error;
    status = wait_request(request);
    if (status != PMAC_OK) { report_error(status, request); goto cleanup; }
    status = *pattern ? pmac_request_copy_match(request, (const uint8_t*)pattern, strlen(pattern), 0, NULL, 0, &required)
                      : pmac_request_copy_response(request, NULL, 0, &required);
    if (status != PMAC_OK && status != PMAC_BUFFER_TOO_SMALL) goto error;
    response = required ? (uint8_t*)malloc(required) : NULL;
    if (required && !response) { fputs("Response allocation failed\n", stderr); goto cleanup; }
    status = *pattern ? pmac_request_copy_match(request, (const uint8_t*)pattern, strlen(pattern), 0, response, required, &required)
                      : pmac_request_copy_response(request, response, required, &required);
    if (status != PMAC_OK) goto error;
    if (required) (void)fwrite(response, 1, required, stdout); /* length, not a NUL-terminated string */
    fputc('\n', stdout);
    failed = 0;
    goto cleanup;
error:
    report_error(status, 0);
cleanup:
    free(response);
    if (request) { status = pmac_request_release(request); if (status != PMAC_OK) { report_error(status, 0); failed = 1; } }
    if (client) {
        status = pmac_client_close(client, 0, 5000);
        if (status == PMAC_OK) status = pmac_client_release(client);
        if (status != PMAC_OK) {
            report_error(status, 0);
            fprintf(stderr, "Retain client=%" PRIu64 " runtime=%" PRIu64 " and retry cleanup in an embedded application\n", client, runtime);
            return 1;
        }
    }
    if (runtime) {
        status = pmac_runtime_shutdown(runtime, 5000);
        if (status == PMAC_OK) status = pmac_runtime_release(runtime);
        if (status != PMAC_OK) {
            report_error(status, 0);
            fprintf(stderr, "Retain runtime=%" PRIu64 " and retry cleanup in an embedded application\n", runtime);
            return 1;
        }
    }
    return failed;
}
