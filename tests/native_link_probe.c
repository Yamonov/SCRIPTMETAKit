#include "scriptmetakit_ffi.h"

int main(void) {
    SmkEngine *engine = 0;
    SmkStatus status = smk_engine_create_default(&engine);
    if (status != SMK_STATUS_OK || !engine) {
        return 1;
    }
    smk_engine_free(engine);
    return 0;
}
