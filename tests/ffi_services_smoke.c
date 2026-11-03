#include "scriptmetakit_ffi.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

static SmkUtf8Slice input(const char *value) {
    SmkUtf8Slice result = {(const uint8_t *)value, strlen(value)};
    return result;
}

static void expect_text(SmkEditResult *result, const char *expected) {
    SmkUtf8Slice text = {0};
    assert(smk_edit_result_text(result, &text) == SMK_STATUS_OK);
    assert(text.len == strlen(expected));
    assert(text.len == 0 || memcmp(text.ptr, expected, text.len) == 0);
    smk_edit_result_free(result);
}

int main(void) {
    SmkEditResult *result = NULL;
    assert(smk_normalize_language_tag(input("EN-us"), &result) == SMK_STATUS_OK);
    expect_text(result, "en-US");
    assert(smk_parse_metadata_tag_list(input("image, image, export,"), &result) == SMK_STATUS_OK);
    expect_text(result, "[\"image\",\"export\"]");
    assert(smk_validate_metadata_tag(input("Description-END"), &result) == SMK_STATUS_OK);
    expect_text(result, "false");
    assert(smk_normalize_language_tag(input("jp"), &result) == SMK_STATUS_INVALID_ARGUMENT);
    assert(result != NULL);
    smk_edit_result_free(result);

    SmkEngine *engine = NULL;
    assert(smk_engine_create_default(&engine) == SMK_STATUS_OK);
    SmkEditResult *defaults = NULL;
    assert(smk_configuration_default_json(&defaults) == SMK_STATUS_OK);
    SmkUtf8Slice configuration = {0};
    assert(smk_edit_result_text(defaults, &configuration) == SMK_STATUS_OK);
    assert(smk_engine_apply_configuration_json(engine, configuration, &result) == SMK_STATUS_OK);
    smk_edit_result_free(result);
    smk_edit_result_free(defaults);
    assert(smk_engine_configuration_json(engine, &result) == SMK_STATUS_OK);
    smk_edit_result_free(result);
    assert(smk_engine_invalidate_cache(engine, 0, &result) == SMK_STATUS_OK);
    expect_text(result, "");
    smk_engine_free(engine);
    puts("C service ABI smoke passed");
    return 0;
}
