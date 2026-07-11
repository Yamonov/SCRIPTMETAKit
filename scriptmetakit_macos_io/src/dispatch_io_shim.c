#include <dispatch/dispatch.h>
#include <fcntl.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef void (*smk_dispatch_io_data_handler)(
    void *context,
    const void *bytes,
    size_t length,
    bool done,
    int error
);

typedef void (*smk_dispatch_io_cleanup_handler)(void *context, int error);

static dispatch_queue_t smk_dispatch_io_queue(void) {
    static dispatch_queue_t queue;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        queue = dispatch_queue_create(
            "com.scriptmetakit.file-io",
            DISPATCH_QUEUE_SERIAL
        );
    });
    return queue;
}

void *smk_dispatch_io_open(
    const char *path,
    size_t high_water,
    void *data_context,
    void *cleanup_context,
    smk_dispatch_io_data_handler data_handler,
    smk_dispatch_io_cleanup_handler cleanup_handler
) {
    dispatch_queue_t queue = smk_dispatch_io_queue();
    dispatch_io_t channel = dispatch_io_create_with_path(
        DISPATCH_IO_RANDOM,
        path,
        O_RDONLY,
        0,
        queue,
        ^(int error) {
            cleanup_handler(cleanup_context, error);
        }
    );
    if (channel == NULL) {
        return NULL;
    }

    dispatch_io_set_high_water(channel, high_water);
    dispatch_io_read(
        channel,
        0,
        SIZE_MAX,
        queue,
        ^(bool done, dispatch_data_t data, int error) {
            if (data != NULL) {
                dispatch_data_apply(
                    data,
                    ^bool(
                        dispatch_data_t region,
                        size_t offset,
                        const void *buffer,
                        size_t size
                    ) {
                        (void)region;
                        (void)offset;
                        if (size > 0) {
                            data_handler(data_context, buffer, size, false, 0);
                        }
                        return true;
                    }
                );
            }
            if (done) {
                data_handler(data_context, NULL, 0, true, error);
            }
        }
    );
    return channel;
}

void smk_dispatch_io_cancel(void *channel) {
    dispatch_io_close((dispatch_io_t)channel, DISPATCH_IO_STOP);
}

void smk_dispatch_io_retain(void *channel) {
    dispatch_retain((dispatch_io_t)channel);
}

void smk_dispatch_io_release(void *channel) {
    dispatch_release((dispatch_io_t)channel);
}
