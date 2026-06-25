package com.arda.input;

import android.content.Context;
import android.os.SystemClock;
import android.view.InputDevice;
import android.view.InputEvent;
import android.view.KeyEvent;
import android.view.MotionEvent;

import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.Locale;

public final class InputServer {
    private static final int INJECT_INPUT_EVENT_MODE_ASYNC = 0;
    private static final int SOURCE_TOUCHSCREEN = InputDevice.SOURCE_TOUCHSCREEN;
    private static final int SOURCE_KEYBOARD = InputDevice.SOURCE_KEYBOARD;

    private final Object inputManager;
    private final Method injectInputEventMethod;
    private final Method setDisplayIdMethod;
    private final int displayId;

    private boolean touchActive;
    private long lastTouchDown;
    private int lastPointerId;
    private float lastX;
    private float lastY;

    // Match Gearhead-style projected touch parameters as closely as possible.
    // This process still uses framework injection, so keep it conservative:
    // no synthetic DOWN for stray MOVE, no UP on stale active touch, and no
    // setDisplayId() call for display 0.
    private static final float ACTIVE_PRESSURE = 0.8f;
    private static final float POINTER_SIZE = 0.05f;

    private InputServer(int displayId) throws Exception {
        this.displayId = displayId;
        this.inputManager = createInputManager();
        Class<?> inputManagerClass = Class.forName("android.hardware.input.InputManager");
        this.injectInputEventMethod = inputManagerClass.getMethod("injectInputEvent", InputEvent.class, int.class);
        Method setDisplayId = null;
        try {
            setDisplayId = InputEvent.class.getMethod("setDisplayId", int.class);
        } catch (NoSuchMethodException ignored) {
            // Android < 10 or hidden on a vendor build. Display 0 still works.
        }
        this.setDisplayIdMethod = setDisplayId;
    }

    public static void main(String[] args) throws Exception {
        int displayId = 0;
        for (int i = 0; i < args.length; i++) {
            if ("--display".equals(args[i]) && i + 1 < args.length) {
                displayId = Integer.parseInt(args[++i]);
            }
        }

        InputServer server = new InputServer(displayId);
        System.err.println("ARDA_INPUTD ready display=" + displayId);
        server.loop();
        System.err.println("ARDA_INPUTD stopped");
    }

    private void loop() throws Exception {
        BufferedReader reader = new BufferedReader(new InputStreamReader(System.in));
        String line;
        while ((line = reader.readLine()) != null) {
            line = line.trim();
            if (line.length() == 0) {
                continue;
            }
            if ("STOP".equals(line) || "BYE".equals(line)) {
                cancelActiveTouch();
                return;
            }
            if ("PING".equals(line)) {
                continue;
            }

            String[] p = line.split("\\s+");
            try {
                if (p.length >= 5 && "T".equals(p[0])) {
                    injectTouch(p[1], Integer.parseInt(p[2]), Integer.parseInt(p[3]), Integer.parseInt(p[4]));
                } else if (p.length >= 2 && "K".equals(p[0])) {
                    injectKeyPress(Integer.parseInt(p[1]));
                } else {
                    System.err.println("ARDA_INPUTD ignored line: " + line);
                }
            } catch (Throwable t) {
                System.err.println("ARDA_INPUTD line failed: " + line + " err=" + t);
            }
        }
    }

    private static Object createInputManager() throws Exception {
        Class<?> cls = Class.forName("android.hardware.input.InputManager");
        try {
            Method getInstance = cls.getDeclaredMethod("getInstance");
            getInstance.setAccessible(true);
            Object manager = getInstance.invoke(null);
            if (manager != null) {
                return manager;
            }
        } catch (Throwable ignored) {
            // Some builds do not expose getInstance(). Try a system Context.
        }

        Context context = createSystemContext();
        Object manager = context.getSystemService(Context.INPUT_SERVICE);
        if (manager == null) {
            throw new IllegalStateException("Context.INPUT_SERVICE returned null");
        }
        return manager;
    }

    private static Context createSystemContext() throws Exception {
        Class<?> activityThreadClass = Class.forName("android.app.ActivityThread");
        Constructor<?> ctor = activityThreadClass.getDeclaredConstructor();
        ctor.setAccessible(true);
        Object activityThread = ctor.newInstance();

        try {
            Field current = activityThreadClass.getDeclaredField("sCurrentActivityThread");
            current.setAccessible(true);
            current.set(null, activityThread);
        } catch (Throwable ignored) {
        }
        try {
            Field systemThread = activityThreadClass.getDeclaredField("mSystemThread");
            systemThread.setAccessible(true);
            systemThread.setBoolean(activityThread, true);
        } catch (Throwable ignored) {
        }

        Method getSystemContext = activityThreadClass.getDeclaredMethod("getSystemContext");
        getSystemContext.setAccessible(true);
        Context context = (Context) getSystemContext.invoke(activityThread);
        if (context == null) {
            throw new IllegalStateException("ActivityThread.getSystemContext() returned null");
        }
        return context;
    }

    private boolean inject(InputEvent event) {
        try {
            if (displayId > 0 && setDisplayIdMethod != null) {
                try {
                    setDisplayIdMethod.invoke(event, displayId);
                } catch (Throwable t) {
                    // Do not fail injection on old/vendor builds where displayId is not supported.
                    System.err.println("ARDA_INPUTD setDisplayId failed: " + t);
                }
            }
            Object result = injectInputEventMethod.invoke(inputManager, event, INJECT_INPUT_EVENT_MODE_ASYNC);
            return Boolean.TRUE.equals(result);
        } catch (Throwable t) {
            System.err.println("ARDA_INPUTD inject failed: " + t);
            return false;
        } finally {
            if (event instanceof MotionEvent) {
                ((MotionEvent) event).recycle();
            }
        }
    }

    private void injectTouch(String actionWord, int x, int y, int pointerId) {
        String action = actionWord.toUpperCase(Locale.US);
        long now = SystemClock.uptimeMillis();
        int motionAction;
        float pressure;

        if ("DOWN".equals(action) || "D".equals(action)) {
            // Do not turn an old active touch into a real click. If the previous
            // gesture was interrupted, cancel it and start clean.
            cancelActiveTouch();
            touchActive = true;
            lastTouchDown = now;
            lastPointerId = pointerId;
            motionAction = MotionEvent.ACTION_DOWN;
            pressure = ACTIVE_PRESSURE;
        } else if ("MOVE".equals(action) || "M".equals(action)) {
            // Gearhead receives a well-formed AAP gesture stream. If we receive
            // a stray MOVE after reconnect/crash, do not synthesize DOWN; that
            // was one source of stale focus/input corruption.
            if (!touchActive) {
                System.err.println("ARDA_INPUTD ignored MOVE without active DOWN x=" + x + " y=" + y);
                return;
            }
            motionAction = MotionEvent.ACTION_MOVE;
            pressure = ACTIVE_PRESSURE;
        } else if ("UP".equals(action) || "U".equals(action)) {
            if (!touchActive) {
                return;
            }
            motionAction = MotionEvent.ACTION_UP;
            pressure = 0.0f;
        } else {
            System.err.println("ARDA_INPUTD unknown touch action: " + actionWord);
            return;
        }

        lastX = x;
        lastY = y;

        boolean ok = injectMotion(motionAction, lastTouchDown, now, pointerId, x, y, pressure);
        if (!ok) {
            System.err.println("ARDA_INPUTD touch failed action=" + action + " x=" + x + " y=" + y + " pointer=" + pointerId);
        }

        if (motionAction == MotionEvent.ACTION_UP) {
            touchActive = false;
        }
    }

    private boolean injectMotion(int motionAction, long downTime, long eventTime, int pointerId, float x, float y, float pressure) {
        MotionEvent.PointerProperties[] properties = new MotionEvent.PointerProperties[1];
        MotionEvent.PointerProperties property = new MotionEvent.PointerProperties();
        property.id = pointerId;
        property.toolType = MotionEvent.TOOL_TYPE_FINGER;
        properties[0] = property;

        MotionEvent.PointerCoords[] coords = new MotionEvent.PointerCoords[1];
        MotionEvent.PointerCoords coord = new MotionEvent.PointerCoords();
        coord.x = x;
        coord.y = y;
        coord.pressure = pressure;
        coord.size = POINTER_SIZE;
        coords[0] = coord;

        MotionEvent event = MotionEvent.obtain(
                downTime,
                eventTime,
                motionAction,
                1,
                properties,
                coords,
                0,
                0,
                1.0f,
                1.0f,
                0,
                0,
                SOURCE_TOUCHSCREEN,
                0);
        return inject(event);
    }

    private void cancelActiveTouch() {
        if (!touchActive) {
            return;
        }
        long now = SystemClock.uptimeMillis();
        injectMotion(MotionEvent.ACTION_CANCEL, lastTouchDown, now, lastPointerId, lastX, lastY, 0.0f);
        touchActive = false;
    }

    private void injectKeyPress(int keyCode) {
        long now = SystemClock.uptimeMillis();
        KeyEvent down = new KeyEvent(now, now, KeyEvent.ACTION_DOWN, keyCode, 0, 0, -1, 0, 0, SOURCE_KEYBOARD);
        KeyEvent up = new KeyEvent(now, now, KeyEvent.ACTION_UP, keyCode, 0, 0, -1, 0, 0, SOURCE_KEYBOARD);
        boolean okDown = inject(down);
        boolean okUp = inject(up);
        if (!(okDown && okUp)) {
            System.err.println("ARDA_INPUTD key failed keycode=" + keyCode);
        }
    }
}
