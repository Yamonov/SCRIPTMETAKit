#include "scriptmetakit_ffi.h"
#include <stdio.h>
#include <string.h>

static int contains(SmkUtf8Slice text, const char *needle) {
    const size_t length = strlen(needle);
    if (length > text.len) return 0;
    for (size_t index = 0; index <= text.len - length; ++index) {
        if (memcmp(text.ptr + index, needle, length) == 0) return 1;
    }
    return 0;
}

int main(int argc, char **argv) {
    SmkEngine *engine = 0;
    SmkStatus status = smk_engine_create_default(&engine);
    if (status != SMK_STATUS_OK || !engine) {
        return 1;
    }
    smk_engine_free(engine);
    if (argc == 3) {
        SmkEditResult *result = NULL;
        SmkUtf8Slice text = {0};
        if (smk_runtime_info_json(&result) != SMK_STATUS_OK || !result) return 2;
        if (smk_edit_result_text(result, &text) != SMK_STATUS_OK) { smk_edit_result_free(result); return 3; }
        char version[128], architecture[128];
        snprintf(version, sizeof(version), "\"package_version\":\"%s\"", argv[1]);
        snprintf(architecture, sizeof(architecture), "\"architecture\":\"%s\"", argv[2]);
        const int valid = contains(text, version) && contains(text, architecture)
            && contains(text, "\"api_version\":1") && contains(text, "\"native_watch\":true")
            && contains(text, "\"http_updates\":true");
        smk_edit_result_free(result);
        if (!valid) return 4;
    }
    return 0;
}
