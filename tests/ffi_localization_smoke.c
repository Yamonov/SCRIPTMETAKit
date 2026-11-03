#include "scriptmetakit_ffi.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

static SmkUtf8Slice text(const char *value) {
    SmkUtf8Slice result = {(const uint8_t *)value, strlen(value)};
    return result;
}

int main(int argc, char **argv) {
    assert(argc == 2);
    SmkEngine *engine = NULL;
    SmkScanResult *result = NULL;
    assert(smk_engine_create_default(&engine) == SMK_STATUS_OK);
    SmkUtf8Slice languages[] = {text("ja"), text("en"), text("fr")};
    uint8_t changed = 0;
    assert(smk_engine_set_preferred_languages(engine, languages, 3, &changed) == SMK_STATUS_OK);
    assert(changed == 1);
    assert(smk_engine_scan_folder(engine, (const uint8_t *)argv[1], strlen(argv[1]), &result) == SMK_STATUS_OK);
    SmkUtf8SliceSlice priorities = {0};
    SmkScriptItemSlice items = {0};
    SmkMetadataPresentationSlice presentations = {0};
    SmkUtf8SliceSlice tags = {0};
    assert(smk_scan_result_preferred_languages(result, &priorities) == SMK_STATUS_OK);
    assert(priorities.len == 3);
    assert(smk_scan_result_items(result, &items) == SMK_STATUS_OK);
    assert(items.len == 4);
    assert(smk_scan_result_metadata_presentations(result, SMK_METADATA_ITEMS, &presentations) == SMK_STATUS_OK);
    assert(presentations.len == items.len);
    assert(smk_scan_result_metadata_tags(result, &tags) == SMK_STATUS_OK);
    for (size_t index = 0; index < items.len; ++index) {
        assert(presentations.ptr[index].tag_count > 0);
        assert(presentations.ptr[index].first_tag_index + presentations.ptr[index].tag_count <= tags.len);
        assert(presentations.ptr[index].name_language.len > 0);
    }
    smk_scan_result_free(result);
    smk_engine_free(engine);
    puts("arm64 C header/ABI language and tag smoke passed");
    return 0;
}
