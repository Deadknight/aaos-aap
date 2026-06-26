package com.arda.input;

import android.content.Context;
import android.content.pm.PackageManager;
import android.media.AudioFormat;
import android.media.AudioRecord;
import android.os.Process;

import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.LinkedHashSet;
import java.util.List;

public final class AudioCaptureServer {
    private static final int AUDIO_SOURCE_DEFAULT = 0;
    private static final int AUDIO_SOURCE_MIC = 1;
    private static final int AUDIO_SOURCE_VOICE_COMMUNICATION = 7;
    private static final int AUDIO_SOURCE_REMOTE_SUBMIX = 8;
    private static final int AUDIO_SOURCE_UNPROCESSED = 9;

    private static final String PERM_RECORD_AUDIO = "android.permission.RECORD_AUDIO";
    private static final String PERM_CAPTURE_AUDIO_OUTPUT = "android.permission.CAPTURE_AUDIO_OUTPUT";

    public static void main(String[] args) throws Exception {
        int sampleRate = 48000;
        int channels = 2;
        int source = AUDIO_SOURCE_REMOTE_SUBMIX;
        String sourceName = "remote_submix";
        String packageName = "com.android.shell";
        int chunkMs = 20;
        boolean legacyFirst = false;
        boolean allowFallbackSources = false;

        for (int i = 0; i < args.length; i++) {
            String arg = args[i];
            if ("--rate".equals(arg) && i + 1 < args.length) {
                sampleRate = Integer.parseInt(args[++i]);
            } else if ("--channels".equals(arg) && i + 1 < args.length) {
                channels = Integer.parseInt(args[++i]);
            } else if ("--source".equals(arg) && i + 1 < args.length) {
                sourceName = args[++i];
                allowFallbackSources = isAutoSource(sourceName);
                source = parseSource(sourceName);
            } else if ("--chunk-ms".equals(arg) && i + 1 < args.length) {
                chunkMs = Integer.parseInt(args[++i]);
            } else if ("--package".equals(arg) && i + 1 < args.length) {
                packageName = args[++i];
            } else if ("--legacy-first".equals(arg) || "--audio-record-legacy-first".equals(arg)) {
                legacyFirst = true;
            } else if ("--fallback-sources".equals(arg) || "--auto-fallback".equals(arg)) {
                allowFallbackSources = true;
            }
        }

        sampleRate = Math.max(8000, Math.min(sampleRate, 96000));
        channels = channels <= 1 ? 1 : 2;
        chunkMs = Math.max(5, Math.min(chunkMs, 200));

        int channelMask = channels == 1
                ? AudioFormat.CHANNEL_IN_MONO
                : AudioFormat.CHANNEL_IN_STEREO;
        int minBuffer = AudioRecord.getMinBufferSize(
                sampleRate,
                channelMask,
                AudioFormat.ENCODING_PCM_16BIT);
        if (minBuffer <= 0) {
            minBuffer = sampleRate * channels * 2 / 2;
        }
        int chunkBytes = Math.max(1, sampleRate * channels * 2 * chunkMs / 1000);
        int bufferBytes = Math.max(minBuffer * 2, chunkBytes * 4);

        Context context = tryCreatePackageContext(packageName);
        printEnvironment(context, packageName, sourceName, source, sampleRate, channels, chunkMs, chunkBytes, minBuffer, bufferBytes);

        AudioRecord record = null;
        Throwable lastFailure = null;
        try {
            List<Integer> sources = buildSourceList(source, allowFallbackSources);
            for (int src : sources) {
                List<String> attempts = new ArrayList<>();
                if (legacyFirst) {
                    attempts.add("legacy");
                    attempts.add("builder-context");
                    attempts.add("builder");
                } else {
                    attempts.add("builder-context");
                    attempts.add("builder");
                    attempts.add("legacy");
                }

                for (String attempt : attempts) {
                    try {
                        record = createRecord(attempt, context, src, sampleRate, channelMask, bufferBytes);
                        if (record.getState() != AudioRecord.STATE_INITIALIZED) {
                            throw new IllegalStateException("AudioRecord not initialized state=" + record.getState());
                        }
                        System.err.println("ARDA_AUDIOCAP create_ok attempt=" + attempt + " source=" + src);
                        runRecord(record, src, sampleRate, channels, chunkMs, chunkBytes, minBuffer, bufferBytes);
                        return;
                    } catch (Throwable t) {
                        lastFailure = t;
                        System.err.println("ARDA_AUDIOCAP create_failed attempt=" + attempt + " source=" + src + " error=" + t);
                        if (record != null) {
                            try { record.release(); } catch (Throwable ignored) {}
                            record = null;
                        }
                    }
                }
            }
            throw new UnsupportedOperationException("Cannot create AudioRecord after all attempts; last=" + lastFailure, lastFailure);
        } catch (Throwable t) {
            System.err.println("ARDA_AUDIOCAP failed: " + t);
            t.printStackTrace(System.err);
            throw t;
        } finally {
            if (record != null) {
                try { record.stop(); } catch (Throwable ignored) {}
                try { record.release(); } catch (Throwable ignored) {}
            }
            System.err.println("ARDA_AUDIOCAP stopped");
        }
    }

    private static AudioRecord createRecord(String attempt, Context context, int source, int sampleRate, int channelMask, int bufferBytes) throws Exception {
        if ("legacy".equals(attempt)) {
            return new AudioRecord(source, sampleRate, channelMask, AudioFormat.ENCODING_PCM_16BIT, bufferBytes);
        }

        AudioFormat format = new AudioFormat.Builder()
                .setSampleRate(sampleRate)
                .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                .setChannelMask(channelMask)
                .build();
        AudioRecord.Builder builder = new AudioRecord.Builder()
                .setAudioSource(source)
                .setAudioFormat(format)
                .setBufferSizeInBytes(bufferBytes);

        if ("builder-context".equals(attempt) && context != null) {
            try {
                Method setContext = AudioRecord.Builder.class.getMethod("setContext", Context.class);
                setContext.invoke(builder, context);
                System.err.println("ARDA_AUDIOCAP builder_context package=" + context.getPackageName());
            } catch (NoSuchMethodException nsme) {
                System.err.println("ARDA_AUDIOCAP builder_context unavailable: setContext missing");
            }
        }
        return builder.build();
    }

    private static void runRecord(AudioRecord record, int source, int sampleRate, int channels, int chunkMs, int chunkBytes, int minBuffer, int bufferBytes) throws Exception {
        System.err.println("ARDA_AUDIOCAP ready source=" + source
                + " rate=" + sampleRate
                + " channels=" + channels
                + " chunk_ms=" + chunkMs
                + " chunk_bytes=" + chunkBytes
                + " min_buffer=" + minBuffer
                + " buffer=" + bufferBytes);

        byte[] buffer = new byte[chunkBytes];
        record.startRecording();
        System.err.println("ARDA_AUDIOCAP started");

        while (true) {
            int offset = 0;
            while (offset < buffer.length) {
                int n = record.read(buffer, offset, buffer.length - offset);
                if (n < 0) {
                    throw new IllegalStateException("AudioRecord.read failed " + n);
                }
                if (n == 0) {
                    Thread.sleep(2);
                    continue;
                }
                offset += n;
            }
            System.out.write(buffer, 0, buffer.length);
            System.out.flush();
        }
    }

    private static void printEnvironment(Context context, String packageName, String sourceName, int source, int sampleRate, int channels, int chunkMs, int chunkBytes, int minBuffer, int bufferBytes) {
        System.err.println("ARDA_AUDIOCAP env uid=" + Process.myUid()
                + " pid=" + Process.myPid()
                + " requested_package=" + packageName
                + " context_package=" + (context == null ? "null" : context.getPackageName())
                + " source_name=" + sourceName
                + " source=" + source
                + " rate=" + sampleRate
                + " channels=" + channels
                + " chunk_ms=" + chunkMs
                + " chunk_bytes=" + chunkBytes
                + " min_buffer=" + minBuffer
                + " buffer=" + bufferBytes);
        if (context != null) {
            System.err.println("ARDA_AUDIOCAP perm record_audio=" + permState(context, PERM_RECORD_AUDIO)
                    + " capture_audio_output=" + permState(context, PERM_CAPTURE_AUDIO_OUTPUT));
        }
    }

    private static String permState(Context context, String permission) {
        try {
            int result = context.checkPermission(permission, Process.myPid(), Process.myUid());
            return result == PackageManager.PERMISSION_GRANTED ? "granted" : "denied(" + result + ")";
        } catch (Throwable t) {
            return "error(" + t + ")";
        }
    }

    private static Context tryCreatePackageContext(String packageName) {
        try {
            Class<?> activityThreadClass = Class.forName("android.app.ActivityThread");
            Object thread = null;
            try {
                Method current = activityThreadClass.getDeclaredMethod("currentActivityThread");
                current.setAccessible(true);
                thread = current.invoke(null);
            } catch (Throwable ignored) {}
            if (thread == null) {
                try {
                    Method systemMain = activityThreadClass.getDeclaredMethod("systemMain");
                    systemMain.setAccessible(true);
                    thread = systemMain.invoke(null);
                } catch (Throwable t) {
                    System.err.println("ARDA_AUDIOCAP context systemMain failed: " + t);
                }
            }
            if (thread == null) {
                System.err.println("ARDA_AUDIOCAP context unavailable: no ActivityThread");
                return null;
            }
            Method getSystemContext = activityThreadClass.getDeclaredMethod("getSystemContext");
            getSystemContext.setAccessible(true);
            Context systemContext = (Context) getSystemContext.invoke(thread);
            if (systemContext == null) {
                System.err.println("ARDA_AUDIOCAP context unavailable: systemContext null");
                return null;
            }
            try {
                Context packageContext = systemContext.createPackageContext(packageName, Context.CONTEXT_IGNORE_SECURITY);
                System.err.println("ARDA_AUDIOCAP context_ok package=" + packageContext.getPackageName());
                return packageContext;
            } catch (Throwable t) {
                System.err.println("ARDA_AUDIOCAP package_context failed package=" + packageName + " error=" + t);
                return systemContext;
            }
        } catch (Throwable t) {
            System.err.println("ARDA_AUDIOCAP context failed: " + t);
            return null;
        }
    }

    private static boolean isAutoSource(String value) {
        if (value == null) {
            return false;
        }
        String v = value.trim().toLowerCase();
        return "auto".equals(v) || "fallback".equals(v) || "probe".equals(v);
    }

    private static List<Integer> buildSourceList(int primary, boolean allowFallbackSources) {
        LinkedHashSet<Integer> set = new LinkedHashSet<>();
        set.add(primary);
        if (allowFallbackSources) {
            set.add(AUDIO_SOURCE_REMOTE_SUBMIX);
            set.add(AUDIO_SOURCE_DEFAULT);
            set.add(AUDIO_SOURCE_VOICE_COMMUNICATION);
            set.add(AUDIO_SOURCE_MIC);
            set.add(AUDIO_SOURCE_UNPROCESSED);
        }
        return new ArrayList<>(set);
    }

    private static int parseSource(String value) {
        if (value == null) {
            return AUDIO_SOURCE_REMOTE_SUBMIX;
        }
        String v = value.trim().toLowerCase();
        if ("remote_submix".equals(v) || "remote-submix".equals(v) || "submix".equals(v) || "mix".equals(v) || "auto".equals(v) || "fallback".equals(v) || "probe".equals(v)) {
            return AUDIO_SOURCE_REMOTE_SUBMIX;
        }
        if ("mic".equals(v) || "microphone".equals(v)) {
            return AUDIO_SOURCE_MIC;
        }
        if ("voice_communication".equals(v) || "voice-communication".equals(v) || "voice".equals(v)) {
            return AUDIO_SOURCE_VOICE_COMMUNICATION;
        }
        if ("unprocessed".equals(v) || "raw".equals(v)) {
            return AUDIO_SOURCE_UNPROCESSED;
        }
        if ("default".equals(v)) {
            return AUDIO_SOURCE_DEFAULT;
        }
        try {
            return Integer.parseInt(v);
        } catch (NumberFormatException ignored) {
            return AUDIO_SOURCE_REMOTE_SUBMIX;
        }
    }
}
