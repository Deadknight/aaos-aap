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
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Set;

public final class InputServer {
    private static final int INJECT_INPUT_EVENT_MODE_ASYNC = 0;
    private static final int SOURCE_TOUCHSCREEN = InputDevice.SOURCE_TOUCHSCREEN;
    private static final int SOURCE_KEYBOARD = InputDevice.SOURCE_KEYBOARD;
    private static final int MAX_POINTERS = 16;
    private static final float ACTIVE_PRESSURE = 0.8f;
    private static final float POINTER_SIZE = 0.05f;

    private final Object inputManager;
    private final Method injectInputEventMethod;
    private final Method setDisplayIdMethod;
    private final int displayId;

    private boolean touchActive;
    private long touchDownTime;
    private List<TouchPoint> lastPointers = new ArrayList<>();
    private final Set<Integer> activePointerIds = new HashSet<>();

    private InputServer(int displayId) throws Exception {
        this.displayId = displayId;
        this.inputManager = createInputManager();
        Class<?> inputManagerClass = inputManager.getClass();
        this.injectInputEventMethod = inputManagerClass.getMethod(
                "injectInputEvent", InputEvent.class, int.class);
        Method setDisplayId = null;
        try {
            setDisplayId = InputEvent.class.getMethod("setDisplayId", int.class);
        } catch (NoSuchMethodException ignored) {
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
        System.err.println("ARDA_INPUTD ready display=" + displayId
                + " protocol=line-v2-multipointer max_pointers=" + MAX_POINTERS);
        server.loop();
        System.err.println("ARDA_INPUTD stopped");
    }

    private void loop() throws Exception {
        BufferedReader reader = new BufferedReader(new InputStreamReader(System.in));
        String line;
        while ((line = reader.readLine()) != null) {
            line = line.trim();
            if (line.isEmpty()) {
                continue;
            }
            if ("STOP".equals(line) || "BYE".equals(line)) {
                cancelActiveTouch("client-stop");
                return;
            }
            if ("PING".equals(line)) {
                continue;
            }

            String[] p = line.split("\\s+");
            try {
                if (p.length >= 4 && "TM".equals(p[0])) {
                    injectMultiTouchLine(p);
                } else if (p.length >= 5 && "T".equals(p[0])) {
                    injectLegacyTouch(
                            p[1],
                            Integer.parseInt(p[2]),
                            Integer.parseInt(p[3]),
                            Integer.parseInt(p[4]));
                } else if (p.length >= 2 && "K".equals(p[0])) {
                    injectKeyPress(Integer.parseInt(p[1]));
                } else {
                    System.err.println("ARDA_INPUTD ignored line: " + line);
                }
            } catch (Throwable t) {
                System.err.println("ARDA_INPUTD line failed: " + line + " err=" + t);
                cancelActiveTouch("malformed-line");
            }
        }
        cancelActiveTouch("stdin-eof");
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
                    System.err.println("ARDA_INPUTD setDisplayId failed: " + t);
                }
            }
            Object result = injectInputEventMethod.invoke(
                    inputManager, event, INJECT_INPUT_EVENT_MODE_ASYNC);
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

    private void injectMultiTouchLine(String[] p) {
        MultiAction action = MultiAction.parse(p[1]);
        int actionIndex = Integer.parseInt(p[2]);
        int pointerCount = Integer.parseInt(p[3]);
        if (pointerCount < 1 || pointerCount > MAX_POINTERS) {
            throw new IllegalArgumentException("pointerCount=" + pointerCount);
        }
        if (p.length != 4 + pointerCount * 3) {
            throw new IllegalArgumentException(
                    "token count=" + p.length + " expected=" + (4 + pointerCount * 3));
        }

        List<TouchPoint> pointers = new ArrayList<>(pointerCount);
        Set<Integer> ids = new HashSet<>();
        int offset = 4;
        for (int i = 0; i < pointerCount; i++) {
            int pointerId = Integer.parseInt(p[offset++]);
            float x = Integer.parseInt(p[offset++]);
            float y = Integer.parseInt(p[offset++]);
            if (pointerId < 0 || pointerId > 31) {
                throw new IllegalArgumentException("pointerId=" + pointerId);
            }
            if (!ids.add(pointerId)) {
                throw new IllegalArgumentException("duplicate pointerId=" + pointerId);
            }
            pointers.add(new TouchPoint(pointerId, x, y));
        }

        injectMultiTouch(action, actionIndex, pointers);
    }

    private void injectMultiTouch(
            MultiAction action,
            int actionIndex,
            List<TouchPoint> pointers) {
        long now = SystemClock.uptimeMillis();
        validateTransition(action, actionIndex, pointers);

        if (action == MultiAction.DOWN) {
            cancelActiveTouch("new-down");
            touchActive = true;
            touchDownTime = now;
        }

        int motionAction = action.toMotionAction(actionIndex);
        boolean ok = injectMotion(motionAction, touchDownTime, now, pointers, action, actionIndex);
        if (!ok) {
            System.err.println("ARDA_INPUTD MULTITOUCH inject failed action=" + action
                    + " index=" + actionIndex + " count=" + pointers.size());
        }

        updateActivePointersAfter(action, actionIndex, pointers);
        System.err.println("ARDA_INPUTD MULTITOUCH action=" + action
                + " index=" + actionIndex
                + " count=" + pointers.size()
                + " active_after=" + activePointerIds.size()
                + " ok=" + ok);
    }

    private void validateTransition(
            MultiAction action,
            int actionIndex,
            List<TouchPoint> pointers) {
        int count = pointers.size();
        if (action.requiresActionIndex()
                && (actionIndex < 0 || actionIndex >= count)) {
            throw new IllegalArgumentException(
                    "actionIndex=" + actionIndex + " count=" + count);
        }

        Set<Integer> eventIds = pointerIdSet(pointers);
        switch (action) {
            case DOWN:
                if (count != 1 || actionIndex != 0) {
                    throw new IllegalArgumentException("DOWN requires one pointer at index 0");
                }
                break;
            case POINTER_DOWN:
                if (!touchActive) {
                    throw new IllegalStateException("POINTER_DOWN without active gesture");
                }
                if (count != activePointerIds.size() + 1) {
                    throw new IllegalArgumentException(
                            "POINTER_DOWN count=" + count
                                    + " active=" + activePointerIds.size());
                }
                Set<Integer> expectedDownBase = new HashSet<>(eventIds);
                expectedDownBase.remove(pointers.get(actionIndex).id);
                if (!expectedDownBase.equals(activePointerIds)) {
                    throw new IllegalArgumentException(
                            "POINTER_DOWN active id mismatch event=" + eventIds
                                    + " active=" + activePointerIds);
                }
                break;
            case MOVE:
                if (!touchActive) {
                    throw new IllegalStateException("MOVE without active gesture");
                }
                if (!eventIds.equals(activePointerIds)) {
                    throw new IllegalArgumentException(
                            "MOVE active id mismatch event=" + eventIds
                                    + " active=" + activePointerIds);
                }
                break;
            case POINTER_UP:
                if (!touchActive) {
                    throw new IllegalStateException("POINTER_UP without active gesture");
                }
                if (count < 2 || !eventIds.equals(activePointerIds)) {
                    throw new IllegalArgumentException(
                            "POINTER_UP id mismatch event=" + eventIds
                                    + " active=" + activePointerIds);
                }
                break;
            case UP:
                if (!touchActive) {
                    throw new IllegalStateException("UP without active gesture");
                }
                if (count != 1 || actionIndex != 0
                        || !eventIds.equals(activePointerIds)) {
                    throw new IllegalArgumentException(
                            "UP id mismatch event=" + eventIds
                                    + " active=" + activePointerIds);
                }
                break;
            case CANCEL:
                if (!touchActive) {
                    throw new IllegalStateException("CANCEL without active gesture");
                }
                break;
        }
    }

    private void updateActivePointersAfter(
            MultiAction action,
            int actionIndex,
            List<TouchPoint> pointers) {
        switch (action) {
            case DOWN:
            case POINTER_DOWN:
            case MOVE:
                activePointerIds.clear();
                activePointerIds.addAll(pointerIdSet(pointers));
                lastPointers = copyPointers(pointers);
                break;
            case POINTER_UP:
                int liftedId = pointers.get(actionIndex).id;
                activePointerIds.clear();
                lastPointers = new ArrayList<>();
                for (TouchPoint pointer : pointers) {
                    if (pointer.id != liftedId) {
                        activePointerIds.add(pointer.id);
                        lastPointers.add(pointer);
                    }
                }
                touchActive = !activePointerIds.isEmpty();
                break;
            case UP:
            case CANCEL:
                touchActive = false;
                activePointerIds.clear();
                lastPointers.clear();
                break;
        }
    }

    private boolean injectMotion(
            int motionAction,
            long downTime,
            long eventTime,
            List<TouchPoint> pointers,
            MultiAction action,
            int actionIndex) {
        int count = pointers.size();
        MotionEvent.PointerProperties[] properties =
                new MotionEvent.PointerProperties[count];
        MotionEvent.PointerCoords[] coords = new MotionEvent.PointerCoords[count];

        for (int i = 0; i < count; i++) {
            TouchPoint point = pointers.get(i);

            MotionEvent.PointerProperties property =
                    new MotionEvent.PointerProperties();
            property.id = point.id;
            property.toolType = MotionEvent.TOOL_TYPE_FINGER;
            properties[i] = property;

            MotionEvent.PointerCoords coord = new MotionEvent.PointerCoords();
            coord.x = point.x;
            coord.y = point.y;
            boolean lifted = (action == MultiAction.UP)
                    || (action == MultiAction.POINTER_UP && i == actionIndex)
                    || action == MultiAction.CANCEL;
            coord.pressure = lifted ? 0.0f : ACTIVE_PRESSURE;
            coord.size = POINTER_SIZE;
            coords[i] = coord;
        }

        MotionEvent event = MotionEvent.obtain(
                downTime,
                eventTime,
                motionAction,
                count,
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

    private void injectLegacyTouch(String actionWord, int x, int y, int pointerId) {
        String action = actionWord.toUpperCase(Locale.US);
        if ("DOWN".equals(action) || "D".equals(action)) {
            injectMultiTouch(
                    MultiAction.DOWN,
                    0,
                    onePointer(pointerId, x, y));
        } else if ("MOVE".equals(action) || "M".equals(action)) {
            injectMultiTouch(
                    MultiAction.MOVE,
                    0,
                    onePointer(pointerId, x, y));
        } else if ("UP".equals(action) || "U".equals(action)) {
            injectMultiTouch(
                    MultiAction.UP,
                    0,
                    onePointer(pointerId, x, y));
        } else {
            throw new IllegalArgumentException("unknown legacy touch action=" + actionWord);
        }
    }

    private void cancelActiveTouch(String reason) {
        if (!touchActive || lastPointers.isEmpty()) {
            touchActive = false;
            activePointerIds.clear();
            lastPointers.clear();
            return;
        }
        long now = SystemClock.uptimeMillis();
        boolean ok = injectMotion(
                MotionEvent.ACTION_CANCEL,
                touchDownTime,
                now,
                lastPointers,
                MultiAction.CANCEL,
                0);
        System.err.println("ARDA_INPUTD MULTITOUCH action=CANCEL reason=" + reason
                + " count=" + lastPointers.size() + " ok=" + ok);
        touchActive = false;
        activePointerIds.clear();
        lastPointers.clear();
    }

    private static List<TouchPoint> onePointer(int id, int x, int y) {
        List<TouchPoint> result = new ArrayList<>(1);
        result.add(new TouchPoint(id, x, y));
        return result;
    }

    private static List<TouchPoint> copyPointers(List<TouchPoint> pointers) {
        return new ArrayList<>(pointers);
    }

    private static Set<Integer> pointerIdSet(List<TouchPoint> pointers) {
        Set<Integer> result = new HashSet<>();
        for (TouchPoint pointer : pointers) {
            result.add(pointer.id);
        }
        return result;
    }

    private void injectKeyPress(int keyCode) {
        long now = SystemClock.uptimeMillis();
        KeyEvent down = new KeyEvent(
                now, now, KeyEvent.ACTION_DOWN, keyCode, 0, 0,
                -1, 0, 0, SOURCE_KEYBOARD);
        KeyEvent up = new KeyEvent(
                now, now, KeyEvent.ACTION_UP, keyCode, 0, 0,
                -1, 0, 0, SOURCE_KEYBOARD);
        boolean okDown = inject(down);
        boolean okUp = inject(up);
        if (!(okDown && okUp)) {
            System.err.println("ARDA_INPUTD key failed keycode=" + keyCode);
        }
    }

    private enum MultiAction {
        DOWN,
        MOVE,
        UP,
        POINTER_DOWN,
        POINTER_UP,
        CANCEL;

        static MultiAction parse(String value) {
            return MultiAction.valueOf(value.toUpperCase(Locale.US));
        }

        boolean requiresActionIndex() {
            return this != MOVE && this != CANCEL;
        }

        int toMotionAction(int actionIndex) {
            switch (this) {
                case DOWN:
                    return MotionEvent.ACTION_DOWN;
                case MOVE:
                    return MotionEvent.ACTION_MOVE;
                case UP:
                    return MotionEvent.ACTION_UP;
                case POINTER_DOWN:
                    return MotionEvent.ACTION_POINTER_DOWN
                            | (actionIndex << MotionEvent.ACTION_POINTER_INDEX_SHIFT);
                case POINTER_UP:
                    return MotionEvent.ACTION_POINTER_UP
                            | (actionIndex << MotionEvent.ACTION_POINTER_INDEX_SHIFT);
                case CANCEL:
                    return MotionEvent.ACTION_CANCEL;
                default:
                    throw new AssertionError(this);
            }
        }
    }

    private static final class TouchPoint {
        final int id;
        final float x;
        final float y;

        TouchPoint(int id, float x, float y) {
            this.id = id;
            this.x = x;
            this.y = y;
        }
    }
}
