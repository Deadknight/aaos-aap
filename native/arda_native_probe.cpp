#include <dlfcn.h>
#include <errno.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdint.h>
#include <string.h>
#include <sys/types.h>
#include <unistd.h>

#if defined(__ANDROID__)
#include <media/NdkMediaCodec.h>
#include <media/NdkMediaFormat.h>
#endif

namespace {

struct Buffer {
    char* data;
    size_t cap;
    size_t len;
};

void append(Buffer* b, const char* fmt, ...) {
    if (!b || !b->data || b->cap == 0 || b->len >= b->cap) return;
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(b->data + b->len, b->cap - b->len, fmt, ap);
    va_end(ap);
    if (n <= 0) return;
    size_t written = static_cast<size_t>(n);
    if (written >= b->cap - b->len) {
        b->len = b->cap - 1;
        b->data[b->len] = '\0';
    } else {
        b->len += written;
    }
}

void probe_library(Buffer* b, const char* name) {
    dlerror();
    void* handle = dlopen(name, RTLD_NOW | RTLD_LOCAL);
    const char* err = dlerror();
    if (handle) {
        append(b, "  %-34s OK\n", name);
        dlclose(handle);
    } else {
        append(b, "  %-34s FAIL %s\n", name, err ? err : "unknown");
    }
}

void probe_symbol(Buffer* b, const char* lib, const char* symbol) {
    dlerror();
    void* handle = dlopen(lib, RTLD_NOW | RTLD_LOCAL);
    const char* open_err = dlerror();
    if (!handle) {
        append(b, "  %-42s FAIL open %s\n", symbol, open_err ? open_err : "unknown");
        return;
    }

    dlerror();
    void* sym = dlsym(handle, symbol);
    const char* sym_err = dlerror();
    if (sym) {
        append(b, "  %-42s OK\n", symbol);
    } else {
        append(b, "  %-42s FAIL %s\n", symbol, sym_err ? sym_err : "not found");
    }
    dlclose(handle);
}

// Android OMX color constants. Public NDK headers do not consistently expose these
// names across API levels, but the integer values are stable.
constexpr int32_t COLOR_FormatYUV420Flexible = 0x7F420888;
constexpr int64_t kDequeueTimeoutUs = 100000;

// Some Android NDK API levels expose the old SYNC_FRAME/key-frame flag
// differently. The numeric value matches MediaCodec BUFFER_FLAG_KEY_FRAME /
// BUFFER_FLAG_SYNC_FRAME. Keep this local so API 23 builds still work.
#ifndef AMEDIACODEC_BUFFER_FLAG_KEY_FRAME
#define AMEDIACODEC_BUFFER_FLAG_KEY_FRAME 1
#endif

#if defined(__ANDROID__)

struct CodecOwner {
    AMediaCodec* codec = nullptr;
    AMediaFormat* format = nullptr;

    ~CodecOwner() {
        if (codec) {
            AMediaCodec_stop(codec);
            AMediaCodec_delete(codec);
        }
        if (format) {
            AMediaFormat_delete(format);
        }
    }
};

void fill_black_yuv420(uint8_t* data, size_t cap, int width, int height) {
    const size_t y_size = static_cast<size_t>(width) * static_cast<size_t>(height);
    const size_t frame_size = y_size + y_size / 2;
    if (!data || cap < frame_size) return;
    memset(data, 0x10, y_size);
    memset(data + y_size, 0x80, frame_size - y_size);
}

int native_encoder_test_android(
    const char* out_path,
    int width,
    int height,
    int bit_rate,
    int fps,
    int seconds,
    Buffer* report
) {
    append(report, "arda native encoder test\n");
    append(report, "pid=%ld uid=%ld euid=%ld\n", static_cast<long>(getpid()), static_cast<long>(getuid()), static_cast<long>(geteuid()));
    append(report, "size=%dx%d bit_rate=%d fps=%d seconds=%d\n", width, height, bit_rate, fps, seconds);
    append(report, "out=%s\n\n", out_path ? out_path : "(null)");

    if (!out_path || out_path[0] == '\0') {
        append(report, "FAIL: missing output path\n");
        return -1;
    }
    if (width <= 0 || height <= 0 || bit_rate <= 0 || fps <= 0 || seconds <= 0) {
        append(report, "FAIL: invalid numeric argument\n");
        return -2;
    }

    const size_t y_size = static_cast<size_t>(width) * static_cast<size_t>(height);
    const size_t frame_size = y_size + y_size / 2;

    FILE* out = fopen(out_path, "wb");
    if (!out) {
        append(report, "FAIL: fopen(%s) errno=%d %s\n", out_path, errno, strerror(errno));
        return -3;
    }

    CodecOwner owner;
    owner.codec = AMediaCodec_createEncoderByType("video/avc");
    if (!owner.codec) {
        append(report, "create encoder: FAIL\n");
        fclose(out);
        return -4;
    }
    append(report, "create encoder: OK\n");

    owner.format = AMediaFormat_new();
    if (!owner.format) {
        append(report, "create format: FAIL\n");
        fclose(out);
        return -5;
    }
    append(report, "create format: OK\n");

    AMediaFormat_setString(owner.format, AMEDIAFORMAT_KEY_MIME, "video/avc");
    AMediaFormat_setInt32(owner.format, AMEDIAFORMAT_KEY_WIDTH, width);
    AMediaFormat_setInt32(owner.format, AMEDIAFORMAT_KEY_HEIGHT, height);
    AMediaFormat_setInt32(owner.format, AMEDIAFORMAT_KEY_COLOR_FORMAT, COLOR_FormatYUV420Flexible);
    AMediaFormat_setInt32(owner.format, AMEDIAFORMAT_KEY_BIT_RATE, bit_rate);
    AMediaFormat_setInt32(owner.format, AMEDIAFORMAT_KEY_FRAME_RATE, fps);
    AMediaFormat_setInt32(owner.format, AMEDIAFORMAT_KEY_I_FRAME_INTERVAL, 1);

    media_status_t st = AMediaCodec_configure(
        owner.codec,
        owner.format,
        nullptr,
        nullptr,
        AMEDIACODEC_CONFIGURE_FLAG_ENCODE
    );
    if (st != AMEDIA_OK) {
        append(report, "configure: FAIL status=%d\n", static_cast<int>(st));
        fclose(out);
        return -6;
    }
    append(report, "configure: OK\n");

    st = AMediaCodec_start(owner.codec);
    if (st != AMEDIA_OK) {
        append(report, "start: FAIL status=%d\n", static_cast<int>(st));
        fclose(out);
        return -7;
    }
    append(report, "start: OK\n");

    const int total_frames = fps * seconds;
    int frames_queued = 0;
    bool input_eos_queued = false;
    bool output_eos_seen = false;
    size_t encoded_bytes = 0;
    int output_buffers = 0;
    int codec_config_buffers = 0;
    int key_buffers = 0;
    int try_again_count = 0;

    while (!output_eos_seen) {
        if (!input_eos_queued) {
            ssize_t in_index = AMediaCodec_dequeueInputBuffer(owner.codec, kDequeueTimeoutUs);
            if (in_index >= 0) {
                if (frames_queued < total_frames) {
                    size_t cap = 0;
                    uint8_t* buf = AMediaCodec_getInputBuffer(owner.codec, static_cast<size_t>(in_index), &cap);
                    if (!buf || cap < frame_size) {
                        append(report, "input buffer too small/null: index=%zd cap=%zu need=%zu\n", in_index, cap, frame_size);
                        fclose(out);
                        return -8;
                    }
                    fill_black_yuv420(buf, cap, width, height);
                    const uint64_t pts = static_cast<uint64_t>(frames_queued) * 1000000ULL / static_cast<uint64_t>(fps);
                    st = AMediaCodec_queueInputBuffer(owner.codec, static_cast<size_t>(in_index), 0, frame_size, pts, 0);
                    if (st != AMEDIA_OK) {
                        append(report, "queue input: FAIL status=%d frame=%d\n", static_cast<int>(st), frames_queued);
                        fclose(out);
                        return -9;
                    }
                    frames_queued++;
                } else {
                    const uint64_t pts = static_cast<uint64_t>(frames_queued) * 1000000ULL / static_cast<uint64_t>(fps);
                    st = AMediaCodec_queueInputBuffer(
                        owner.codec,
                        static_cast<size_t>(in_index),
                        0,
                        0,
                        pts,
                        AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM
                    );
                    if (st != AMEDIA_OK) {
                        append(report, "queue input EOS: FAIL status=%d\n", static_cast<int>(st));
                        fclose(out);
                        return -10;
                    }
                    input_eos_queued = true;
                }
            }
        }

        AMediaCodecBufferInfo info;
        memset(&info, 0, sizeof(info));
        ssize_t out_index = AMediaCodec_dequeueOutputBuffer(owner.codec, &info, kDequeueTimeoutUs);
        if (out_index == AMEDIACODEC_INFO_TRY_AGAIN_LATER) {
            try_again_count++;
            if (input_eos_queued && try_again_count > 100) {
                append(report, "dequeue output: timeout after EOS\n");
                fclose(out);
                return -11;
            }
            continue;
        }
        try_again_count = 0;

        if (out_index == AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED) {
            AMediaFormat* output_format = AMediaCodec_getOutputFormat(owner.codec);
            const char* fmt_str = output_format ? AMediaFormat_toString(output_format) : nullptr;
            append(report, "output format changed: %s\n", fmt_str ? fmt_str : "(null)");
            if (output_format) AMediaFormat_delete(output_format);
            continue;
        }
        if (out_index < 0) {
            append(report, "dequeue output: info=%zd\n", out_index);
            continue;
        }

        size_t cap = 0;
        uint8_t* out_buf = AMediaCodec_getOutputBuffer(owner.codec, static_cast<size_t>(out_index), &cap);
        if (out_buf && info.size > 0 && static_cast<size_t>(info.offset + info.size) <= cap) {
            const uint8_t* start = out_buf + info.offset;
            size_t written = fwrite(start, 1, static_cast<size_t>(info.size), out);
            if (written != static_cast<size_t>(info.size)) {
                append(report, "fwrite: short write errno=%d %s\n", errno, strerror(errno));
                fclose(out);
                return -12;
            }
            encoded_bytes += written;
            output_buffers++;
            if (info.flags & AMEDIACODEC_BUFFER_FLAG_CODEC_CONFIG) codec_config_buffers++;
            if (info.flags & AMEDIACODEC_BUFFER_FLAG_KEY_FRAME) key_buffers++;
        }

        if (info.flags & AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM) {
            output_eos_seen = true;
        }
        AMediaCodec_releaseOutputBuffer(owner.codec, static_cast<size_t>(out_index), false);
    }

    fflush(out);
    fclose(out);

    append(report, "\nresult:\n");
    append(report, "  frames_queued=%d\n", frames_queued);
    append(report, "  output_buffers=%d\n", output_buffers);
    append(report, "  codec_config_buffers=%d\n", codec_config_buffers);
    append(report, "  key_buffers=%d\n", key_buffers);
    append(report, "  encoded_bytes=%zu\n", encoded_bytes);
    append(report, "  output_eos_seen=%s\n", output_eos_seen ? "yes" : "no");

    return encoded_bytes > 0 ? 0 : -13;
}

#endif // __ANDROID__

} // namespace

extern "C" int arda_native_probe(char* out, size_t out_len) {
    if (!out || out_len == 0) return -1;
    out[0] = '\0';
    Buffer b{out, out_len, 0};

    append(&b, "arda native probe\n");
    append(&b, "pid=%ld uid=%ld euid=%ld\n", static_cast<long>(getpid()), static_cast<long>(getuid()), static_cast<long>(geteuid()));

    errno = 0;
    int sr = access("/system/bin/screenrecord", X_OK);
    append(&b, "screenrecord_exec=%s", sr == 0 ? "OK" : "FAIL");
    if (sr != 0) append(&b, " errno=%d %s", errno, strerror(errno));
    append(&b, "\n\n");

    append(&b, "dlopen libraries:\n");
    probe_library(&b, "libmediandk.so");
    probe_library(&b, "libmedia.so");
    probe_library(&b, "libstagefright.so");
    probe_library(&b, "libstagefright_foundation.so");
    probe_library(&b, "libgui.so");
    probe_library(&b, "libui.so");
    probe_library(&b, "libbinder.so");
    probe_library(&b, "libutils.so");
    probe_library(&b, "libandroid_runtime.so");

    append(&b, "\npublic NDK MediaCodec symbols in libmediandk.so:\n");
    probe_symbol(&b, "libmediandk.so", "AMediaCodec_createEncoderByType");
    probe_symbol(&b, "libmediandk.so", "AMediaCodec_createInputSurface");
    probe_symbol(&b, "libmediandk.so", "AMediaCodec_configure");
    probe_symbol(&b, "libmediandk.so", "AMediaCodec_start");
    probe_symbol(&b, "libmediandk.so", "AMediaCodec_dequeueOutputBuffer");
    probe_symbol(&b, "libmediandk.so", "AMediaCodec_getOutputBuffer");
    probe_symbol(&b, "libmediandk.so", "AMediaCodec_releaseOutputBuffer");
    probe_symbol(&b, "libmediandk.so", "AMediaFormat_new");
    probe_symbol(&b, "libmediandk.so", "AMediaFormat_setString");
    probe_symbol(&b, "libmediandk.so", "AMediaFormat_setInt32");

    append(&b, "\nmeaning:\n");
    append(&b, "  libmediandk OK + AMediaCodec_createInputSurface OK => encoder-surface path is available.\n");
    append(&b, "  libgui/libbinder/libui OK => private SurfaceFlinger-style capture may be reachable, but headers/symbols still need matching.\n");

    return 0;
}

extern "C" int arda_native_encoder_test(
    const char* out_path,
    int width,
    int height,
    int bit_rate,
    int fps,
    int seconds,
    char* report,
    size_t report_len
) {
    if (!report || report_len == 0) return -1;
    report[0] = '\0';
    Buffer b{report, report_len, 0};
#if defined(__ANDROID__)
    return native_encoder_test_android(out_path, width, height, bit_rate, fps, seconds, &b);
#else
    append(&b, "native encoder test is only available on Android targets\n");
    return -99;
#endif
}
