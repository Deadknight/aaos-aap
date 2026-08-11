package com.arda.inputbridge;

import android.app.Application;
import android.app.ActivityManager;
import android.content.Context;
import android.content.Intent;
import android.hardware.input.InputManager;
import android.net.Credentials;
import android.net.LocalServerSocket;
import android.net.LocalSocket;
import android.os.Process;
import android.os.SystemClock;
import android.os.UserHandle;
import android.util.Log;
import android.view.InputDevice;
import android.view.InputEvent;
import android.view.KeyEvent;
import android.view.MotionEvent;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.PrintWriter;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Locale;
import java.util.Set;

public final class ArdaInputApplication extends Application {
    private static final String TAG = "ArdaInputBridge";
    private static final String SOCKET_NAME = "arda_input_bridge";
    private static final int ALLOWED_UID = 0;
    private static final int MAX_POINTERS = 16;
    private static final int INJECT_INPUT_EVENT_MODE_ASYNC = 0;
    private static final float ACTIVE_PRESSURE = 0.8f;
    private static final float POINTER_SIZE = 0.05f;

    @Override
    public void onCreate() {
        super.onCreate();
        int userId = Process.myUid() / 100000;
        if (userId != 0) {
            Log.i(TAG, "disabled for secondary user=" + userId + " uid=" + Process.myUid());
            return;
        }

        Thread thread = new Thread(this::runServer, "arda-input-bridge");
        thread.setDaemon(true);
        thread.start();
    }

    private void runServer() {
        while (true) {
            try (LocalServerSocket server = new LocalServerSocket(SOCKET_NAME)) {
                Log.i(TAG, "READY transport=unix-abstract name=" + SOCKET_NAME
                        + " allowed_uid=" + ALLOWED_UID
                        + " uid=" + Process.myUid()
                        + " protocol=line-v2-multipointer"
                        + " normalizer=snapshot-reconcile-v1"
                        + " max_pointers=" + MAX_POINTERS);
                while (true) {
                    LocalSocket client = server.accept();
                    Credentials credentials = client.getPeerCredentials();
                    int peerUid = credentials != null ? credentials.getUid() : -1;
                    int peerPid = credentials != null ? credentials.getPid() : -1;
                    if (peerUid != ALLOWED_UID) {
                        Log.w(TAG, "rejected peer uid=" + peerUid + " pid=" + peerPid);
                        closeQuietly(client);
                        continue;
                    }

                    Log.i(TAG, "client accepted uid=" + peerUid + " pid=" + peerPid);
                    try {
                        handleClient(client);
                    } catch (Throwable t) {
                        Log.e(TAG, "client failed", t);
                    } finally {
                        closeQuietly(client);
                        Log.i(TAG, "client disconnected");
                    }
                }
            } catch (Throwable t) {
                Log.e(TAG, "server failed; retrying transport=unix-abstract name=" + SOCKET_NAME, t);
                SystemClock.sleep(1000);
            }
        }
    }

    private void handleClient(LocalSocket client) throws Exception {
        BufferedReader reader = new BufferedReader(
                new InputStreamReader(client.getInputStream()));
        PrintWriter writer = new PrintWriter(client.getOutputStream(), true);
        FrameworkInjector injector = new FrameworkInjector(this);

        try {
            String line;
            while ((line = reader.readLine()) != null) {
                line = line.trim();
                if (line.isEmpty()) {
                    continue;
                }
                if ("STOP".equals(line) || "BYE".equals(line)) {
                    return;
                }
                if ("PING".equals(line)) {
                    writer.println("PONG");
                    continue;
                }

                String[] parts = line.split("\\s+");
                try {
                    if (parts.length >= 2 && "DISPLAY".equals(parts[0])) {
                        injector.setDisplayId(Integer.parseInt(parts[1]));
                    } else if (parts.length >= 4 && "TM".equals(parts[0])) {
                        injector.injectMultiTouchLine(parts);
                    } else if (parts.length >= 5 && "T".equals(parts[0])) {
                        injector.injectLegacyTouch(
                                parts[1],
                                Integer.parseInt(parts[2]),
                                Integer.parseInt(parts[3]),
                                Integer.parseInt(parts[4]));
                    } else if (parts.length >= 5 && "K2".equals(parts[0])) {
                        injector.injectKeyEvent(
                                Integer.parseInt(parts[2]),
                                "DOWN".equals(parts[1]),
                                Integer.parseInt(parts[3]),
                                !"0".equals(parts[4]));
                    } else if (parts.length >= 2 && "S".equals(parts[0])) {
                        injector.injectSemanticAction(parts[1]);
                    } else if (parts.length >= 2 && "K".equals(parts[0])) {
                        injector.injectKeyPress(Integer.parseInt(parts[1]));
                    } else {
                        Log.w(TAG, "ignored line=" + line);
                    }
                } catch (Throwable t) {
                    Log.e(TAG, "line failed line=" + line, t);
                    injector.cancelActiveTouch("malformed-line");
                }
            }
        } finally {
            injector.cancelActiveTouch("client-disconnect");
        }
    }

    private static void closeQuietly(LocalSocket socket) {
        try {
            socket.close();
        } catch (IOException ignored) {
        }
    }

    private static final class FrameworkInjector {
        private static final int SOURCE_TOUCHSCREEN = InputDevice.SOURCE_TOUCHSCREEN;
        private static final int SOURCE_KEYBOARD = InputDevice.SOURCE_KEYBOARD;

        private final Context context;
        private final Object inputManager;
        private final Map<Integer, Long> keyDownTimes = new HashMap<>();
        private final Method injectInputEventMethod;
        private final Method setDisplayIdMethod;

        private int displayId;
        private boolean touchActive;
        private long touchDownTime;
        private final Set<Integer> activePointerIds = new HashSet<>();
        private List<TouchPoint> lastPointers = new ArrayList<>();
        private long moveLogCounter;

        FrameworkInjector(Context context) throws Exception {
            this.context = context;
            Object manager = null;
            Method injectMethod = null;

            try {
                Class<?> globalClass = Class.forName("android.hardware.input.InputManagerGlobal");
                Method getInstance = globalClass.getMethod("getInstance");
                manager = getInstance.invoke(null);
                injectMethod = globalClass.getMethod(
                        "injectInputEvent", InputEvent.class, int.class);
            } catch (Throwable globalFailure) {
                InputManager fallback = context.getSystemService(InputManager.class);
                if (fallback == null) {
                    throw new IllegalStateException("InputManager unavailable", globalFailure);
                }
                manager = fallback;
                injectMethod = fallback.getClass().getMethod(
                        "injectInputEvent", InputEvent.class, int.class);
            }

            this.inputManager = manager;
            this.injectInputEventMethod = injectMethod;

            Method setDisplayId = null;
            try {
                setDisplayId = InputEvent.class.getMethod("setDisplayId", int.class);
            } catch (NoSuchMethodException ignored) {
            }
            this.setDisplayIdMethod = setDisplayId;
        }

        void setDisplayId(int displayId) {
            if (this.displayId != displayId) {
                cancelActiveTouch("display-change");
                this.displayId = displayId;
            }
            Log.i(TAG, "DISPLAY id=" + displayId);
        }

        void injectMultiTouchLine(String[] parts) {
            MultiAction action = MultiAction.parse(parts[1]);
            int actionIndex = Integer.parseInt(parts[2]);
            int pointerCount = Integer.parseInt(parts[3]);
            if (pointerCount < 1 || pointerCount > MAX_POINTERS) {
                throw new IllegalArgumentException("pointerCount=" + pointerCount);
            }
            int expectedParts = 4 + pointerCount * 3;
            if (parts.length != expectedParts) {
                throw new IllegalArgumentException(
                        "token count=" + parts.length + " expected=" + expectedParts);
            }

            List<TouchPoint> pointers = new ArrayList<>(pointerCount);
            Set<Integer> ids = new HashSet<>();
            int offset = 4;
            for (int i = 0; i < pointerCount; i++) {
                int pointerId = Integer.parseInt(parts[offset++]);
                float x = Integer.parseInt(parts[offset++]);
                float y = Integer.parseInt(parts[offset++]);
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

        void injectLegacyTouch(String actionWord, int x, int y, int pointerId) {
            String action = actionWord.toUpperCase(Locale.US);
            List<TouchPoint> pointer = onePointer(pointerId, x, y);
            if ("DOWN".equals(action) || "D".equals(action)) {
                injectMultiTouch(MultiAction.DOWN, 0, pointer);
            } else if ("MOVE".equals(action) || "M".equals(action)) {
                injectMultiTouch(MultiAction.MOVE, 0, pointer);
            } else if ("UP".equals(action) || "U".equals(action)) {
                injectMultiTouch(MultiAction.UP, 0, pointer);
            } else {
                throw new IllegalArgumentException("unknown legacy action=" + actionWord);
            }
        }

        void injectMultiTouch(
                MultiAction action,
                int actionIndex,
                List<TouchPoint> pointers) {
            if (pointers.isEmpty()) {
                throw new IllegalArgumentException("multipointer frame has no pointers");
            }
            if (action.requiresActionIndex()
                    && (actionIndex < 0 || actionIndex >= pointers.size())) {
                Log.w(TAG, "RECONCILE invalid action index action=" + action
                        + " index=" + actionIndex
                        + " count=" + pointers.size()
                        + " fallback=0");
                actionIndex = 0;
            }

            switch (action) {
                case DOWN:
                    cancelActiveTouch("incoming-down-reset");
                    bootstrapFromSnapshot(pointers, "incoming-down");
                    break;
                case POINTER_DOWN:
                    reconcileSnapshot(pointers, "incoming-pointer-down", true);
                    break;
                case MOVE:
                    reconcileSnapshot(pointers, "incoming-move", true);
                    break;
                case POINTER_UP:
                case UP:
                    normalizeLift(action, actionIndex, pointers);
                    break;
                case CANCEL:
                    cancelActiveTouch("incoming-cancel");
                    break;
            }
        }

        /**
         * Reconciles the Android gesture state from the complete pointer snapshot
         * carried by AAP. DHU implementations do not always deliver a perfectly
         * Android-ordered DOWN/POINTER_DOWN/... sequence. In particular, a MOVE
         * may be the first frame that contains the second pointer, and final
         * releases can arrive as UP(count > 1) followed by POINTER_UP. Rather than
         * rejecting the entire gesture, synthesize the missing Android transitions.
         */
        private void reconcileSnapshot(
                List<TouchPoint> incoming,
                String reason,
                boolean emitMove) {
            if (!touchActive || lastPointers.isEmpty()) {
                Log.i(TAG, "RECONCILE bootstrap reason=" + reason
                        + " incoming_ids=" + pointerIdSet(incoming));
                bootstrapFromSnapshot(incoming, reason + "-bootstrap");
                if (emitMove && touchActive) {
                    emitNormalized(
                            MultiAction.MOVE,
                            0,
                            copyPointers(incoming),
                            reason + "-post-bootstrap-move");
                }
                return;
            }

            Set<Integer> incomingIds = pointerIdSet(incoming);

            // Remove pointers that disappeared from a snapshot without a clean
            // POINTER_UP. Use the last known complete event so Android receives
            // the lifted pointer in the POINTER_UP frame.
            List<Integer> removedIds = new ArrayList<>();
            for (TouchPoint active : lastPointers) {
                if (!incomingIds.contains(active.id)) {
                    removedIds.add(active.id);
                }
            }
            for (Integer removedId : removedIds) {
                List<TouchPoint> eventPointers = mergeCoordinates(lastPointers, incoming);
                int removedIndex = indexOfId(eventPointers, removedId);
                if (removedIndex < 0) {
                    continue;
                }
                Log.i(TAG, "RECONCILE synthesize-lift reason=" + reason
                        + " pointer_id=" + removedId
                        + " active_before=" + pointerIdSet(eventPointers));
                if (eventPointers.size() == 1) {
                    emitNormalized(
                            MultiAction.UP,
                            0,
                            eventPointers,
                            reason + "-missing-pointer-up-final");
                } else {
                    emitNormalized(
                            MultiAction.POINTER_UP,
                            removedIndex,
                            eventPointers,
                            reason + "-missing-pointer-up");
                }
            }

            // Add pointers that appeared in a MOVE/POINTER_DOWN snapshot without
            // a usable preceding POINTER_DOWN. This is the sequence observed from
            // DHU and is the main reason the strict v1 bridge rejected every MOVE.
            for (TouchPoint incomingPointer : incoming) {
                if (containsId(lastPointers, incomingPointer.id)) {
                    continue;
                }
                List<TouchPoint> eventPointers = composeEventOrderWithNewPointer(
                        incoming, incomingPointer.id);
                int addedIndex = indexOfId(eventPointers, incomingPointer.id);
                Log.i(TAG, "RECONCILE synthesize-pointer-down reason=" + reason
                        + " pointer_id=" + incomingPointer.id
                        + " index=" + addedIndex
                        + " event_ids=" + pointerIdSet(eventPointers));
                if (!touchActive || lastPointers.isEmpty()) {
                    bootstrapFromSnapshot(eventPointers, reason + "-late-bootstrap");
                } else {
                    emitNormalized(
                            MultiAction.POINTER_DOWN,
                            addedIndex,
                            eventPointers,
                            reason + "-missing-pointer-down");
                }
            }

            if (emitMove && touchActive) {
                Set<Integer> activeIds = pointerIdSet(lastPointers);
                if (activeIds.equals(incomingIds)) {
                    emitNormalized(
                            MultiAction.MOVE,
                            0,
                            copyPointers(incoming),
                            reason + "-normalized-move");
                } else {
                    Log.w(TAG, "RECONCILE move snapshot mismatch after repair reason="
                            + reason
                            + " incoming=" + incomingIds
                            + " active=" + activeIds
                            + " action=cancel-and-bootstrap");
                    cancelActiveTouch(reason + "-unrecoverable-mismatch");
                    bootstrapFromSnapshot(incoming, reason + "-rebootstrap");
                }
            }
        }

        private void normalizeLift(
                MultiAction incomingAction,
                int actionIndex,
                List<TouchPoint> incoming) {
            int liftedId = incoming.get(actionIndex).id;

            if (!touchActive || lastPointers.isEmpty()) {
                Log.w(TAG, "RECONCILE release-without-active ignored"
                        + " incoming_action=" + incomingAction
                        + " lifted_id=" + liftedId
                        + " incoming_ids=" + pointerIdSet(incoming));
                return;
            }

            List<TouchPoint> eventPointers = mergeCoordinates(lastPointers, incoming);
            int activeLiftIndex = indexOfId(eventPointers, liftedId);

            // A release snapshot can contain stale pointers that were already
            // lifted. Never re-add them during release processing.
            if (activeLiftIndex < 0) {
                if (eventPointers.size() == 1) {
                    liftedId = eventPointers.get(0).id;
                    activeLiftIndex = 0;
                    Log.i(TAG, "RECONCILE release action-index fallback"
                            + " incoming_action=" + incomingAction
                            + " fallback_pointer_id=" + liftedId);
                } else {
                    Log.i(TAG, "RECONCILE duplicate-release ignored"
                            + " incoming_action=" + incomingAction
                            + " lifted_id=" + liftedId
                            + " active_ids=" + pointerIdSet(eventPointers));
                    return;
                }
            }

            // Normalize UP(count > 1) to POINTER_UP and normalize a final
            // POINTER_UP to ACTION_UP. This matches Android's required event
            // sequence even when DHU sends the two release action names reversed.
            if (eventPointers.size() == 1) {
                emitNormalized(
                        MultiAction.UP,
                        0,
                        eventPointers,
                        "incoming-" + incomingAction + "-normalized-final-up");
            } else {
                emitNormalized(
                        MultiAction.POINTER_UP,
                        activeLiftIndex,
                        eventPointers,
                        "incoming-" + incomingAction + "-normalized-pointer-up");
            }
        }

        private void bootstrapFromSnapshot(
                List<TouchPoint> snapshot,
                String reason) {
            if (snapshot.isEmpty()) {
                return;
            }
            if (touchActive) {
                cancelActiveTouch(reason + "-bootstrap-reset");
            }

            List<TouchPoint> current = new ArrayList<>();
            current.add(snapshot.get(0));
            emitNormalized(MultiAction.DOWN, 0, current, reason + "-down");

            for (int i = 1; i < snapshot.size(); i++) {
                current = new ArrayList<>(current);
                current.add(snapshot.get(i));
                emitNormalized(
                        MultiAction.POINTER_DOWN,
                        current.size() - 1,
                        current,
                        reason + "-pointer-down-" + i);
            }
        }

        private void emitNormalized(
                MultiAction action,
                int actionIndex,
                List<TouchPoint> pointers,
                String reason) {
            if (pointers.isEmpty()) {
                throw new IllegalArgumentException("normalized event has no pointers");
            }
            if (action.requiresActionIndex()
                    && (actionIndex < 0 || actionIndex >= pointers.size())) {
                throw new IllegalArgumentException(
                        "normalized actionIndex=" + actionIndex
                                + " count=" + pointers.size()
                                + " action=" + action);
            }

            long now = SystemClock.uptimeMillis();
            if (action == MultiAction.DOWN) {
                touchDownTime = now;
            }

            int motionAction = action.toMotionAction(actionIndex);
            boolean ok = injectMotion(
                    motionAction,
                    touchDownTime,
                    now,
                    pointers,
                    action,
                    actionIndex);
            if (!ok) {
                throw new IllegalStateException(
                        "injectInputEvent returned false normalized_action=" + action
                                + " index=" + actionIndex
                                + " count=" + pointers.size()
                                + " ids=" + pointerIdSet(pointers)
                                + " reason=" + reason);
            }

            updateStateAfterNormalized(action, actionIndex, pointers);
            logNormalized(action, actionIndex, pointers, reason, ok);
        }

        private void updateStateAfterNormalized(
                MultiAction action,
                int actionIndex,
                List<TouchPoint> pointers) {
            switch (action) {
                case DOWN:
                case POINTER_DOWN:
                case MOVE:
                    setActivePointers(pointers);
                    break;
                case POINTER_UP:
                    int liftedId = pointers.get(actionIndex).id;
                    List<TouchPoint> remaining = new ArrayList<>();
                    for (TouchPoint pointer : pointers) {
                        if (pointer.id != liftedId) {
                            remaining.add(pointer);
                        }
                    }
                    setActivePointers(remaining);
                    break;
                case UP:
                case CANCEL:
                    clearTouchState();
                    break;
            }
        }

        private void logNormalized(
                MultiAction action,
                int actionIndex,
                List<TouchPoint> pointers,
                String reason,
                boolean ok) {
            if (action == MultiAction.MOVE) {
                moveLogCounter++;
                if (moveLogCounter <= 8 || moveLogCounter % 25 == 0) {
                    Log.d(TAG, "NORMALIZED action=MOVE"
                            + " count=" + pointers.size()
                            + " ids=" + pointerIdSet(pointers)
                            + " move_event=" + moveLogCounter
                            + " reason=" + reason
                            + " ok=" + ok);
                }
            } else {
                moveLogCounter = 0;
                Log.i(TAG, "NORMALIZED action=" + action
                        + " index=" + actionIndex
                        + " count=" + pointers.size()
                        + " ids=" + pointerIdSet(pointers)
                        + " active_after=" + activePointerIds.size()
                        + " reason=" + reason
                        + " ok=" + ok);
            }
        }

        private void setActivePointers(List<TouchPoint> pointers) {
            lastPointers = copyPointers(pointers);
            activePointerIds.clear();
            activePointerIds.addAll(pointerIdSet(lastPointers));
            touchActive = !lastPointers.isEmpty();
        }

        private void clearTouchState() {
            touchActive = false;
            activePointerIds.clear();
            lastPointers.clear();
        }

        private List<TouchPoint> mergeCoordinates(
                List<TouchPoint> base,
                List<TouchPoint> updates) {
            List<TouchPoint> result = new ArrayList<>(base.size());
            for (TouchPoint active : base) {
                TouchPoint update = findById(updates, active.id);
                result.add(update != null ? update : active);
            }
            return result;
        }

        private List<TouchPoint> composeEventOrderWithNewPointer(
                List<TouchPoint> incoming,
                int newPointerId) {
            List<TouchPoint> result = new ArrayList<>();
            Set<Integer> emitted = new HashSet<>();

            // Prefer the upstream order for IDs that are already active and for
            // the new pointer. Pointer IDs remain the stable identity.
            for (TouchPoint pointer : incoming) {
                if (pointer.id == newPointerId || containsId(lastPointers, pointer.id)) {
                    if (emitted.add(pointer.id)) {
                        result.add(pointer);
                    }
                }
            }
            // Preserve any active pointer omitted from the incoming ordering so
            // the synthesized POINTER_DOWN remains a complete Android event.
            for (TouchPoint active : lastPointers) {
                if (emitted.add(active.id)) {
                    result.add(active);
                }
            }
            return result;
        }

        private static TouchPoint findById(
                List<TouchPoint> pointers,
                int pointerId) {
            for (TouchPoint pointer : pointers) {
                if (pointer.id == pointerId) {
                    return pointer;
                }
            }
            return null;
        }

        private static boolean containsId(
                List<TouchPoint> pointers,
                int pointerId) {
            return findById(pointers, pointerId) != null;
        }

        private static int indexOfId(
                List<TouchPoint> pointers,
                int pointerId) {
            for (int i = 0; i < pointers.size(); i++) {
                if (pointers.get(i).id == pointerId) {
                    return i;
                }
            }
            return -1;
        }

        private static List<TouchPoint> copyPointers(
                List<TouchPoint> pointers) {
            return new ArrayList<>(pointers);
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
            MotionEvent.PointerCoords[] coords =
                    new MotionEvent.PointerCoords[count];

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
                boolean lifted = action == MultiAction.UP
                        || action == MultiAction.CANCEL
                        || (action == MultiAction.POINTER_UP && i == actionIndex);
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

        private boolean inject(InputEvent event) {
            try {
                if (displayId > 0 && setDisplayIdMethod != null) {
                    try {
                        setDisplayIdMethod.invoke(event, displayId);
                    } catch (Throwable t) {
                        Log.w(TAG, "setDisplayId failed id=" + displayId, t);
                    }
                }
                Object result = injectInputEventMethod.invoke(
                        inputManager, event, INJECT_INPUT_EVENT_MODE_ASYNC);
                return Boolean.TRUE.equals(result);
            } catch (Throwable t) {
                Log.e(TAG, "injectInputEvent failed", t);
                return false;
            } finally {
                if (event instanceof MotionEvent) {
                    ((MotionEvent) event).recycle();
                }
            }
        }

        void injectKeyEvent(int keyCode, boolean down, int metaState, boolean longPress) {
            long now = SystemClock.uptimeMillis();
            long downTime;
            if (down) {
                Long existing = keyDownTimes.get(keyCode);
                if (existing == null) {
                    existing = now;
                    keyDownTimes.put(keyCode, existing);
                }
                downTime = existing;
            } else {
                Long existing = keyDownTimes.remove(keyCode);
                downTime = existing != null ? existing : now;
            }

            int action = down ? KeyEvent.ACTION_DOWN : KeyEvent.ACTION_UP;
            int repeatCount = longPress && down ? 1 : 0;
            int flags = longPress && down ? KeyEvent.FLAG_LONG_PRESS : 0;
            KeyEvent event = new KeyEvent(
                    downTime, now, action, keyCode, repeatCount, metaState,
                    -1, 0, flags, SOURCE_KEYBOARD);
            boolean ok = inject(event);
            Log.i(TAG, "KEY action=" + (down ? "DOWN" : "UP")
                    + " keycode=" + keyCode
                    + " meta=" + metaState
                    + " longpress=" + longPress
                    + " display=" + displayId
                    + " ok=" + ok);
            if (!ok) {
                throw new IllegalStateException(
                        "injectInputEvent returned false keycode=" + keyCode);
            }
        }

        void injectSemanticAction(String actionWord) {
            String action = actionWord.toUpperCase(Locale.US);
            Intent intent;
            if ("MEDIA".equals(action)) {
                intent = Intent.makeMainSelectorActivity(
                        Intent.ACTION_MAIN, Intent.CATEGORY_APP_MUSIC);
            } else if ("NAVIGATION".equals(action)) {
                intent = Intent.makeMainSelectorActivity(
                        Intent.ACTION_MAIN, Intent.CATEGORY_APP_MAPS);
            } else if ("TEL".equals(action)) {
                intent = new Intent(Intent.ACTION_DIAL);
            } else {
                throw new IllegalArgumentException(
                        "unknown semantic action=" + actionWord);
            }

            intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK
                    | Intent.FLAG_ACTIVITY_CLEAR_TOP);
            int userId = ActivityManager.getCurrentUser();
            context.startActivityAsUser(intent, UserHandle.of(userId));
            Log.i(TAG, "SEMANTIC action=" + action
                    + " user=" + userId
                    + " dispatch=PASS");
        }

        void cancelActiveTouch(String reason) {
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
            Log.i(TAG, "MULTITOUCH action=CANCEL reason=" + reason
                    + " count=" + lastPointers.size() + " ok=" + ok);
            clearTouchState();
        }

        void injectKeyPress(int keyCode) {
            long now = SystemClock.uptimeMillis();
            KeyEvent down = new KeyEvent(
                    now, now, KeyEvent.ACTION_DOWN, keyCode, 0, 0,
                    -1, 0, 0, SOURCE_KEYBOARD);
            KeyEvent up = new KeyEvent(
                    now, now, KeyEvent.ACTION_UP, keyCode, 0, 0,
                    -1, 0, 0, SOURCE_KEYBOARD);
            boolean downOk = inject(down);
            boolean upOk = inject(up);
            if (!(downOk && upOk)) {
                Log.e(TAG, "key injection failed keycode=" + keyCode);
            }
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

    private static List<TouchPoint> onePointer(int id, int x, int y) {
        List<TouchPoint> result = new ArrayList<>(1);
        result.add(new TouchPoint(id, x, y));
        return result;
    }

    private static Set<Integer> pointerIdSet(List<TouchPoint> pointers) {
        Set<Integer> result = new HashSet<>();
        for (TouchPoint pointer : pointers) {
            result.add(pointer.id);
        }
        return result;
    }
}
