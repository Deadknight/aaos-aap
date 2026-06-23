#include <dlfcn.h>
#include <errno.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdint.h>
#include <string.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>
#include <time.h>

#if defined(__ANDROID__)
#include <android/native_window.h>
#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <GLES2/gl2.h>
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
constexpr int32_t COLOR_FormatSurface = 0x7F000789;
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


struct NativeWindowOwner {
    ANativeWindow* window = nullptr;
    ~NativeWindowOwner() {
        if (window) {
            ANativeWindow_release(window);
        }
    }
};

struct EglOwner {
    EGLDisplay display = EGL_NO_DISPLAY;
    EGLContext context = EGL_NO_CONTEXT;
    EGLSurface surface = EGL_NO_SURFACE;

    ~EglOwner() {
        if (display != EGL_NO_DISPLAY) {
            eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT);
            if (surface != EGL_NO_SURFACE) eglDestroySurface(display, surface);
            if (context != EGL_NO_CONTEXT) eglDestroyContext(display, context);
            eglTerminate(display);
        }
    }
};

typedef EGLBoolean (*EglPresentationTimeAndroidFn)(EGLDisplay display, EGLSurface surface, khronos_stime_nanoseconds_t time);

// These functions exist on this AAOS runtime, but the public NDK headers mark them
// as introduced in API 26. We build with API 23 for now, so calling them directly
// fails at compile time. Resolve them dynamically instead.
typedef media_status_t (*AMediaCodecCreateInputSurfaceFn)(AMediaCodec* codec, ANativeWindow** window);
typedef media_status_t (*AMediaCodecSignalEndOfInputStreamFn)(AMediaCodec* codec);

struct MediaNdkRuntimeApi {
    void* handle = nullptr;
    AMediaCodecCreateInputSurfaceFn create_input_surface = nullptr;
    AMediaCodecSignalEndOfInputStreamFn signal_end_of_input_stream = nullptr;
};

MediaNdkRuntimeApi load_media_ndk_runtime_api(Buffer* report) {
    MediaNdkRuntimeApi api;
    dlerror();
    api.handle = dlopen("libmediandk.so", RTLD_NOW | RTLD_LOCAL);
    const char* open_err = dlerror();
    if (!api.handle) {
        append(report, "dlopen libmediandk.so for API26 symbols: FAIL %s\n", open_err ? open_err : "unknown");
        return api;
    }

    api.create_input_surface = reinterpret_cast<AMediaCodecCreateInputSurfaceFn>(
        dlsym(api.handle, "AMediaCodec_createInputSurface")
    );
    api.signal_end_of_input_stream = reinterpret_cast<AMediaCodecSignalEndOfInputStreamFn>(
        dlsym(api.handle, "AMediaCodec_signalEndOfInputStream")
    );

    append(report, "runtime AMediaCodec_createInputSurface: %s\n", api.create_input_surface ? "OK" : "missing");
    append(report, "runtime AMediaCodec_signalEndOfInputStream: %s\n", api.signal_end_of_input_stream ? "OK" : "missing");
    return api;
}

int drain_codec_output_to_file(
    AMediaCodec* codec,
    FILE* out,
    Buffer* report,
    bool wait_for_buffer,
    bool* output_eos_seen,
    int* output_buffers,
    int* codec_config_buffers,
    int* key_buffers,
    size_t* encoded_bytes
) {
    const int64_t timeout = wait_for_buffer ? kDequeueTimeoutUs : 0;
    while (true) {
        AMediaCodecBufferInfo info;
        memset(&info, 0, sizeof(info));
        ssize_t out_index = AMediaCodec_dequeueOutputBuffer(codec, &info, timeout);

        if (out_index == AMEDIACODEC_INFO_TRY_AGAIN_LATER) {
            return 0;
        }
        if (out_index == AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED) {
            AMediaFormat* output_format = AMediaCodec_getOutputFormat(codec);
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
        uint8_t* out_buf = AMediaCodec_getOutputBuffer(codec, static_cast<size_t>(out_index), &cap);
        if (out_buf && info.size > 0 && static_cast<size_t>(info.offset + info.size) <= cap) {
            const uint8_t* start = out_buf + info.offset;
            size_t written = fwrite(start, 1, static_cast<size_t>(info.size), out);
            if (written != static_cast<size_t>(info.size)) {
                append(report, "fwrite: short write errno=%d %s\n", errno, strerror(errno));
                AMediaCodec_releaseOutputBuffer(codec, static_cast<size_t>(out_index), false);
                return -1;
            }
            *encoded_bytes += written;
            (*output_buffers)++;
            if (info.flags & AMEDIACODEC_BUFFER_FLAG_CODEC_CONFIG) (*codec_config_buffers)++;
            if (info.flags & AMEDIACODEC_BUFFER_FLAG_KEY_FRAME) (*key_buffers)++;
        }

        if (info.flags & AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM) {
            *output_eos_seen = true;
        }
        AMediaCodec_releaseOutputBuffer(codec, static_cast<size_t>(out_index), false);

        if (!wait_for_buffer) {
            // Non-blocking drain mode: keep draining immediately available buffers, then return.
            continue;
        }
        if (*output_eos_seen) return 0;
    }
}

int setup_egl_for_window(ANativeWindow* window, int width, int height, EglOwner* egl, Buffer* report) {
    egl->display = eglGetDisplay(EGL_DEFAULT_DISPLAY);
    if (egl->display == EGL_NO_DISPLAY) {
        append(report, "eglGetDisplay: FAIL err=0x%x\n", eglGetError());
        return -1;
    }
    if (!eglInitialize(egl->display, nullptr, nullptr)) {
        append(report, "eglInitialize: FAIL err=0x%x\n", eglGetError());
        return -2;
    }
    append(report, "eglInitialize: OK\n");

    const EGLint config_attribs[] = {
        EGL_RENDERABLE_TYPE, EGL_OPENGL_ES2_BIT,
        EGL_SURFACE_TYPE, EGL_WINDOW_BIT,
        EGL_RED_SIZE, 8,
        EGL_GREEN_SIZE, 8,
        EGL_BLUE_SIZE, 8,
        EGL_ALPHA_SIZE, 8,
        EGL_NONE
    };
    EGLConfig config = nullptr;
    EGLint num_configs = 0;
    if (!eglChooseConfig(egl->display, config_attribs, &config, 1, &num_configs) || num_configs <= 0) {
        append(report, "eglChooseConfig: FAIL err=0x%x num=%d\n", eglGetError(), static_cast<int>(num_configs));
        return -3;
    }
    append(report, "eglChooseConfig: OK\n");

    const EGLint context_attribs[] = { EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE };
    egl->context = eglCreateContext(egl->display, config, EGL_NO_CONTEXT, context_attribs);
    if (egl->context == EGL_NO_CONTEXT) {
        append(report, "eglCreateContext: FAIL err=0x%x\n", eglGetError());
        return -4;
    }
    append(report, "eglCreateContext: OK\n");

    egl->surface = eglCreateWindowSurface(egl->display, config, window, nullptr);
    if (egl->surface == EGL_NO_SURFACE) {
        append(report, "eglCreateWindowSurface: FAIL err=0x%x\n", eglGetError());
        return -5;
    }
    append(report, "eglCreateWindowSurface: OK\n");

    if (!eglMakeCurrent(egl->display, egl->surface, egl->surface, egl->context)) {
        append(report, "eglMakeCurrent: FAIL err=0x%x\n", eglGetError());
        return -6;
    }
    append(report, "eglMakeCurrent: OK\n");

    glViewport(0, 0, width, height);
    GLenum glerr = glGetError();
    if (glerr != GL_NO_ERROR) {
        append(report, "glViewport: glError=0x%x\n", glerr);
    }
    return 0;
}

int native_surface_encoder_test_android(
    const char* out_path,
    int width,
    int height,
    int bit_rate,
    int fps,
    int seconds,
    Buffer* report
) {
    append(report, "arda native surface encoder test\n");
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
    AMediaFormat_setInt32(owner.format, AMEDIAFORMAT_KEY_COLOR_FORMAT, COLOR_FormatSurface);
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
        append(report, "configure surface encoder: FAIL status=%d\n", static_cast<int>(st));
        fclose(out);
        return -6;
    }
    append(report, "configure surface encoder: OK\n");

    MediaNdkRuntimeApi media_api = load_media_ndk_runtime_api(report);
    if (!media_api.create_input_surface || !media_api.signal_end_of_input_stream) {
        append(report, "FAIL: required surface encoder runtime symbols are missing\n");
        fclose(out);
        return -7;
    }

    NativeWindowOwner window_owner;
    st = media_api.create_input_surface(owner.codec, &window_owner.window);
    if (st != AMEDIA_OK || !window_owner.window) {
        append(report, "create input surface: FAIL status=%d window=%p\n", static_cast<int>(st), window_owner.window);
        fclose(out);
        return -8;
    }
    append(report, "create input surface: OK window=%p\n", window_owner.window);

    EglOwner egl;
    int egl_status = setup_egl_for_window(window_owner.window, width, height, &egl, report);
    if (egl_status != 0) {
        fclose(out);
        return -20 + egl_status;
    }

    st = AMediaCodec_start(owner.codec);
    if (st != AMEDIA_OK) {
        append(report, "start: FAIL status=%d\n", static_cast<int>(st));
        fclose(out);
        return -8;
    }
    append(report, "start: OK\n");

    void* egl_handle = dlopen("libEGL.so", RTLD_NOW | RTLD_LOCAL);
    EglPresentationTimeAndroidFn presentation_time = nullptr;
    if (egl_handle) {
        presentation_time = reinterpret_cast<EglPresentationTimeAndroidFn>(dlsym(egl_handle, "eglPresentationTimeANDROID"));
    }
    append(report, "eglPresentationTimeANDROID: %s\n", presentation_time ? "OK" : "missing/nonfatal");

    const int total_frames = fps * seconds;
    int frames_rendered = 0;
    bool output_eos_seen = false;
    size_t encoded_bytes = 0;
    int output_buffers = 0;
    int codec_config_buffers = 0;
    int key_buffers = 0;

    for (int frame = 0; frame < total_frames; frame++) {
        float r = static_cast<float>((frame * 37) % 255) / 255.0f;
        float g = static_cast<float>((frame * 73) % 255) / 255.0f;
        float bl = static_cast<float>((frame * 17) % 255) / 255.0f;
        glClearColor(r, g, bl, 1.0f);
        glClear(GL_COLOR_BUFFER_BIT);
        GLenum glerr = glGetError();
        if (glerr != GL_NO_ERROR) {
            append(report, "glClear frame=%d glError=0x%x\n", frame, glerr);
            fclose(out);
            return -9;
        }

        if (presentation_time) {
            const khronos_stime_nanoseconds_t pts_ns = static_cast<khronos_stime_nanoseconds_t>(frame) * 1000000000LL / static_cast<khronos_stime_nanoseconds_t>(fps);
            presentation_time(egl.display, egl.surface, pts_ns);
        }

        if (!eglSwapBuffers(egl.display, egl.surface)) {
            append(report, "eglSwapBuffers frame=%d FAIL err=0x%x\n", frame, eglGetError());
            fclose(out);
            return -10;
        }
        frames_rendered++;

        int drain_status = drain_codec_output_to_file(
            owner.codec, out, report, false, &output_eos_seen,
            &output_buffers, &codec_config_buffers, &key_buffers, &encoded_bytes
        );
        if (drain_status != 0) {
            fclose(out);
            return -11;
        }
        struct timespec ts;
        ts.tv_sec = 0;
        ts.tv_nsec = 1000000000L / fps;
        nanosleep(&ts, nullptr);
    }

    st = media_api.signal_end_of_input_stream(owner.codec);
    if (st != AMEDIA_OK) {
        append(report, "signalEndOfInputStream: FAIL status=%d\n", static_cast<int>(st));
        fclose(out);
        return -12;
    }
    append(report, "signalEndOfInputStream: OK\n");

    int wait_loops = 0;
    while (!output_eos_seen && wait_loops < 200) {
        int drain_status = drain_codec_output_to_file(
            owner.codec, out, report, true, &output_eos_seen,
            &output_buffers, &codec_config_buffers, &key_buffers, &encoded_bytes
        );
        if (drain_status != 0) {
            fclose(out);
            return -13;
        }
        wait_loops++;
    }

    fflush(out);
    fclose(out);
    if (egl_handle) dlclose(egl_handle);

    append(report, "\nresult:\n");
    append(report, "  frames_rendered=%d\n", frames_rendered);
    append(report, "  output_buffers=%d\n", output_buffers);
    append(report, "  codec_config_buffers=%d\n", codec_config_buffers);
    append(report, "  key_buffers=%d\n", key_buffers);
    append(report, "  encoded_bytes=%zu\n", encoded_bytes);
    append(report, "  output_eos_seen=%s\n", output_eos_seen ? "yes" : "no");

    return encoded_bytes > 0 ? 0 : -14;
}

#endif // __ANDROID__

#if defined(__ANDROID__)

// Tiny ABI-compatible stand-in for android::sp<T>. On Android 11/aarch64,
// sp<T> stores a single strong-reference pointer. We intentionally make this
// type non-trivial so C++ uses the same indirect-return ABI as android::sp<T>
// for functions such as SurfaceComposerClient::createDisplay(). We release the
// reference manually via RefBase::decStrong after each private call.
struct RawSp {
    void* ptr;
    RawSp() : ptr(nullptr) {}
    RawSp(const RawSp& other) : ptr(other.ptr) {}
    ~RawSp() {}
};

using String8CtorFn = void (*)(void* self, const char* value);
using String8DtorFn = void (*)(void* self);
using RefBaseDecStrongFn = void (*)(const void* self, const void* id);
using SccGetInternalDisplayTokenFn = RawSp (*)();
using SccCreateDisplayFn = RawSp (*)(const void* name, bool secure);
using SccDestroyDisplayFn = void (*)(const RawSp& display);

void release_raw_sp(Buffer* report, const char* label, RawSp* sp, RefBaseDecStrongFn dec_strong) {
    (void)dec_strong;
    if (!sp || !sp->ptr) {
        append(report, "release %-28s skipped/null\n", label ? label : "(null)");
        return;
    }

    // Do NOT call RefBase::decStrong directly here. The value in sp<T> is a T*
    // (for example IBinder*), while RefBase may be a base subobject with a
    // different address because of multiple inheritance. Calling decStrong on
    // the unadjusted pointer can SIGBUS/SIGSEGV on Android's private ABI.
    // This active probe is short-lived, so intentionally leaking these temporary
    // strong refs is safer than crashing. Later capture code should either keep
    // the display token for the whole process lifetime or use a real C++ build
    // against matching platform headers.
    append(report, "release %-28s skipped/private-sp-leak ptr=%p\n", label ? label : "(null)", sp->ptr);
    sp->ptr = nullptr;
}

void* must_sym(Buffer* report, void* handle, const char* label, const char* symbol) {
    if (!handle) {
        append(report, "  %-36s FAIL no-handle\n", label);
        return nullptr;
    }
    dlerror();
    void* sym = dlsym(handle, symbol);
    const char* err = dlerror();
    if (sym) {
        append(report, "  %-36s OK\n", label);
    } else {
        append(report, "  %-36s FAIL %s\n", label, err ? err : "not found");
    }
    return sym;
}

static const char* kDisplayActiveLogPath = "/data/local/tmp/arda-display-active-probe.log";

void log_file_reset() {
    FILE* f = fopen(kDisplayActiveLogPath, "wb");
    if (f) fclose(f);
}

void log_file_append(const char* fmt, ...) {
    FILE* f = fopen(kDisplayActiveLogPath, "ab");
    if (!f) return;
    va_list ap;
    va_start(ap, fmt);
    vfprintf(f, fmt, ap);
    va_end(ap);
    fflush(f);
    fsync(fileno(f));
    fclose(f);
}

void append_file_to_report(Buffer* report, const char* path) {
    FILE* f = fopen(path, "rb");
    if (!f) {
        append(report, "could not read child log %s errno=%d %s\n", path, errno, strerror(errno));
        return;
    }
    char buf[1024];
    size_t n = 0;
    while ((n = fread(buf, 1, sizeof(buf) - 1, f)) > 0) {
        buf[n] = '\0';
        append(report, "%s", buf);
    }
    fclose(f);
}

int native_display_active_probe_android_child(Buffer* report) {
    append(report, "arda native display active probe child\n");
    log_file_append("arda native display active probe child\n");
    append(report, "pid=%ld uid=%ld euid=%ld\n\n", static_cast<long>(getpid()), static_cast<long>(getuid()), static_cast<long>(geteuid()));
    log_file_append("pid=%ld uid=%ld euid=%ld\n\n", static_cast<long>(getpid()), static_cast<long>(getuid()), static_cast<long>(geteuid()));

    void* libgui = dlopen("libgui.so", RTLD_NOW | RTLD_LOCAL);
    void* libutils = dlopen("libutils.so", RTLD_NOW | RTLD_LOCAL);
    append(report, "dlopen:\n");
    log_file_append("dlopen...\n");
    append(report, "  %-36s %s\n", "libgui.so", libgui ? "OK" : "FAIL");
    append(report, "  %-36s %s\n\n", "libutils.so", libutils ? "OK" : "FAIL");
    if (!libgui || !libutils) {
        if (libgui) dlclose(libgui);
        if (libutils) dlclose(libutils);
        return -1;
    }

    append(report, "resolve private ABI calls:\n");
    log_file_append("resolve private ABI calls...\n");
    auto string8_ctor = reinterpret_cast<String8CtorFn>(must_sym(report, libutils, "String8(char const*)", "_ZN7android7String8C1EPKc"));
    auto string8_dtor = reinterpret_cast<String8DtorFn>(must_sym(report, libutils, "~String8", "_ZN7android7String8D1Ev"));
    auto dec_strong = reinterpret_cast<RefBaseDecStrongFn>(must_sym(report, libutils, "RefBase::decStrong", "_ZNK7android7RefBase9decStrongEPKv"));
    auto get_internal = reinterpret_cast<SccGetInternalDisplayTokenFn>(must_sym(report, libgui, "getInternalDisplayToken", "_ZN7android21SurfaceComposerClient23getInternalDisplayTokenEv"));
    auto create_display = reinterpret_cast<SccCreateDisplayFn>(must_sym(report, libgui, "createDisplay", "_ZN7android21SurfaceComposerClient13createDisplayERKNS_7String8Eb"));
    auto destroy_display = reinterpret_cast<SccDestroyDisplayFn>(must_sym(report, libgui, "destroyDisplay", "_ZN7android21SurfaceComposerClient14destroyDisplayERKNS_2spINS_7IBinderEEE"));

    if (!string8_ctor || !string8_dtor || !get_internal || !create_display || !destroy_display) {
        append(report, "\nFAIL: missing required symbol; not calling private API.\n");
        dlclose(libgui);
        dlclose(libutils);
        return -2;
    }

    append(report, "\nactive calls:\n");
    log_file_append("active calls...\n");

    RawSp internal_display;
    RawSp virtual_display;

    // String8 is one pointer on this Android branch, but keep extra room so the
    // private constructor can safely initialize its real object layout if needed.
    alignas(16) unsigned char name_storage[64];
    memset(name_storage, 0, sizeof(name_storage));

    bool name_constructed = false;
    log_file_append("before String8 ctor\n");
    string8_ctor(name_storage, "arda-native-display-probe");
    log_file_append("after String8 ctor\n");
    name_constructed = true;
    append(report, "  String8 ctor                      OK\n");

    log_file_append("before getInternalDisplayToken\n");
    internal_display = get_internal();
    log_file_append("after getInternalDisplayToken ptr=%p\n", internal_display.ptr);
    append(report, "  getInternalDisplayToken           ptr=%p %s\n", internal_display.ptr, internal_display.ptr ? "OK" : "NULL");

    log_file_append("before createDisplay secure=false\n");
    virtual_display = create_display(name_storage, false);
    log_file_append("after createDisplay ptr=%p\n", virtual_display.ptr);
    append(report, "  createDisplay secure=false        ptr=%p %s\n", virtual_display.ptr, virtual_display.ptr ? "OK" : "NULL");

    if (virtual_display.ptr) {
        log_file_append("before destroyDisplay\n");
        destroy_display(virtual_display);
        log_file_append("after destroyDisplay\n");
        append(report, "  destroyDisplay                    OK\n");
    } else {
        append(report, "  destroyDisplay                    skipped/null\n");
    }

    log_file_append("before release virtual display skip-safe\n");
    release_raw_sp(report, "virtual display", &virtual_display, dec_strong);
    log_file_append("after release virtual display skip-safe\n");
    log_file_append("before release internal display skip-safe\n");
    release_raw_sp(report, "internal display", &internal_display, dec_strong);
    log_file_append("after release internal display skip-safe\n");

    if (name_constructed) {
        log_file_append("before String8 dtor\n");
        string8_dtor(name_storage);
        log_file_append("after String8 dtor\n");
        append(report, "  String8 dtor                      OK\n");
    }

    dlclose(libgui);
    dlclose(libutils);

    append(report, "\nmeaning:\n");
    append(report, "  If this did not crash and createDisplay returned OK, the private sp<String8>/IBinder ABI is callable.\n");
    append(report, "  Next step can attempt Transaction::setDisplaySurface against the MediaCodec input surface. This probe intentionally skips direct decStrong on private sp<T> pointers to avoid pointer-adjustment crashes.\n");

    return virtual_display.ptr == nullptr ? 0 : 0;
}


int native_display_active_probe_android(Buffer* report) {
    append(report, "arda native display active probe parent\n");
    append(report, "pid=%ld uid=%ld euid=%ld\n", static_cast<long>(getpid()), static_cast<long>(getuid()), static_cast<long>(geteuid()));
    append(report, "child_log=%s\n\n", kDisplayActiveLogPath);

    log_file_reset();

    pid_t pid = fork();
    if (pid < 0) {
        append(report, "fork: FAIL errno=%d %s\n", errno, strerror(errno));
        return -50;
    }

    if (pid == 0) {
        char child_buf[32 * 1024];
        child_buf[0] = '\0';
        Buffer child_report{child_buf, sizeof(child_buf), 0};
        int rc = native_display_active_probe_android_child(&child_report);
        log_file_append("\nchild returned rc=%d\n", rc);
        _exit(rc == 0 ? 0 : 100);
    }

    int status = 0;
    pid_t waited = waitpid(pid, &status, 0);
    if (waited < 0) {
        append(report, "waitpid: FAIL errno=%d %s\n", errno, strerror(errno));
        append(report, "\nchild log so far:\n");
        append_file_to_report(report, kDisplayActiveLogPath);
        return -51;
    }

    append(report, "child_status_raw=%d\n", status);
    if (WIFEXITED(status)) {
        append(report, "child_exited code=%d\n", WEXITSTATUS(status));
    }
    if (WIFSIGNALED(status)) {
        append(report, "child_signaled signal=%d\n", WTERMSIG(status));
    }

    append(report, "\nchild log:\n");
    append_file_to_report(report, kDisplayActiveLogPath);

    append(report, "\nmeaning:\n");
    append(report, "  Parent wrapper prevents an empty adb output if a private ABI call crashes.\n");
    append(report, "  The child log shows the last completed step before any crash/abort. v9c also skips direct RefBase::decStrong release calls.\n");

    return WIFEXITED(status) && WEXITSTATUS(status) == 0 ? 0 : -52;
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
    probe_library(&b, "libEGL.so");
    probe_library(&b, "libGLESv2.so");
    probe_library(&b, "libandroid.so");

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
    probe_symbol(&b, "libmediandk.so", "AMediaCodec_signalEndOfInputStream");
    probe_symbol(&b, "libEGL.so", "eglPresentationTimeANDROID");

    append(&b, "\nmeaning:\n");
    append(&b, "  libmediandk OK + AMediaCodec_createInputSurface OK => encoder-surface path is available.\n");
    append(&b, "  libgui/libbinder/libui OK => private SurfaceFlinger-style capture may be reachable, but headers/symbols still need matching.\n");

    return 0;
}


extern "C" int arda_native_display_probe(char* out, size_t out_len) {
    if (!out || out_len == 0) return -1;
    out[0] = '\0';
    Buffer b{out, out_len, 0};

    append(&b, "arda native display probe\n");
    append(&b, "pid=%ld uid=%ld euid=%ld\n\n", static_cast<long>(getpid()), static_cast<long>(getuid()), static_cast<long>(geteuid()));

    append(&b, "dlopen display/capture libraries:\n");
    probe_library(&b, "libgui.so");
    probe_library(&b, "libbinder.so");
    probe_library(&b, "libui.so");
    probe_library(&b, "libutils.so");
    probe_library(&b, "libnativewindow.so");
    probe_library(&b, "libsurfaceflinger_client.so");

    append(&b, "\nlibgui SurfaceComposerClient display symbols:\n");
    probe_symbol(&b, "libgui.so", "_ZN7android21SurfaceComposerClientC1Ev");
    probe_symbol(&b, "libgui.so", "_ZN7android21SurfaceComposerClientC2Ev");
    probe_symbol(&b, "libgui.so", "_ZN7android21SurfaceComposerClientD1Ev");
    probe_symbol(&b, "libgui.so", "_ZN7android21SurfaceComposerClientD2Ev");
    probe_symbol(&b, "libgui.so", "_ZN7android21SurfaceComposerClient13createDisplayERKNS_7String8Eb");
    probe_symbol(&b, "libgui.so", "_ZN7android21SurfaceComposerClient14destroyDisplayERKNS_2spINS_7IBinderEEE");
    probe_symbol(&b, "libgui.so", "_ZN7android21SurfaceComposerClient23getInternalDisplayTokenEv");
    probe_symbol(&b, "libgui.so", "_ZN7android21SurfaceComposerClient20getInternalDisplayIdEv");
    probe_symbol(&b, "libgui.so", "_ZN7android21SurfaceComposerClient21getPhysicalDisplayIdsEv");
    probe_symbol(&b, "libgui.so", "_ZN7android21SurfaceComposerClient23getPhysicalDisplayTokenEm");

    append(&b, "\nlibgui Transaction display-routing symbols:\n");
    probe_symbol(&b, "libgui.so", "_ZN7android21SurfaceComposerClient11Transaction17setDisplaySurfaceERKNS_2spINS_7IBinderEEERKNS2_INS_22IGraphicBufferProducerEEE");
    probe_symbol(&b, "libgui.so", "_ZN7android21SurfaceComposerClient11Transaction20setDisplayProjectionERKNS_2spINS_7IBinderEEENS_2ui8RotationERKNS_4RectESB_");
    probe_symbol(&b, "libgui.so", "_ZN7android21SurfaceComposerClient11Transaction20setDisplayLayerStackERKNS_2spINS_7IBinderEEEj");
    probe_symbol(&b, "libgui.so", "_ZN7android21SurfaceComposerClient11Transaction14setDisplaySizeERKNS_2spINS_7IBinderEEEjj");
    probe_symbol(&b, "libgui.so", "_ZN7android21SurfaceComposerClient11Transaction5applyEb");

    append(&b, "\nlibgui surface/mirror/capture symbols:\n");
    probe_symbol(&b, "libgui.so", "_ZNK7android14SurfaceControl25getIGraphicBufferProducerEv");
    probe_symbol(&b, "libgui.so", "_ZNK7android14SurfaceControl10getSurfaceEv");
    probe_symbol(&b, "libgui.so", "_ZN7android7SurfaceC1ERKNS_2spINS_22IGraphicBufferProducerEEEb");
    probe_symbol(&b, "libgui.so", "_ZN7android21SurfaceComposerClient13mirrorSurfaceEPNS_14SurfaceControlE");
    probe_symbol(&b, "libgui.so", "_ZN7android16ScreenshotClient7captureERKNS_2spINS_7IBinderEEENS_8hardware8graphics6common4V1_29DataspaceENS9_11PixelFormatERKNS_4RectEjjbNS_2ui8RotationEbPNS1_INS_13GraphicBufferEEERb");
    probe_symbol(&b, "libgui.so", "_ZN7android16ScreenshotClient7captureERKNS_2spINS_7IBinderEEENS_8hardware8graphics6common4V1_29DataspaceENS9_11PixelFormatERKNS_4RectEjjbNS_2ui8RotationEPNS1_INS_13GraphicBufferEEE");
    probe_symbol(&b, "libgui.so", "_ZN7android16ScreenshotClient7captureEmPNS_8hardware8graphics6common4V1_29DataspaceEPNS_2spINS_13GraphicBufferEEE");

    append(&b, "\nmeaning:\n");
    append(&b, "  createDisplay + setDisplaySurface + apply => virtual-display to encoder surface path is symbolically present.\n");
    append(&b, "  getInternalDisplayToken + ScreenshotClient capture => fallback screenshot path is also symbolically present.\n");
    append(&b, "  This command only checks ABI/symbol reachability; it does not yet call private constructors.\n");

    return 0;
}



extern "C" int arda_native_display_active_probe(char* out, size_t out_len) {
    if (!out || out_len == 0) return -1;
    out[0] = '\0';
    Buffer b{out, out_len, 0};
#if defined(__ANDROID__)
    return native_display_active_probe_android(&b);
#else
    append(&b, "native display active probe is only available on Android targets\n");
    return -99;
#endif
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


extern "C" int arda_native_surface_encoder_test(
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
    return native_surface_encoder_test_android(out_path, width, height, bit_rate, fps, seconds, &b);
#else
    append(&b, "native surface encoder test is only available on Android targets\n");
    return -99;
#endif
}
