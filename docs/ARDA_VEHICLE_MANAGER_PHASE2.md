# ARDA Vehicle Manager — Phase 2

## Status

Phase 1.1 runtime gates are closed:

```text
sensor-gate-smoke                 PASS
sensor-wasm-smoke                 PASS
vehicle-policy-smoke              PASS
vehicle-router-smoke arbitration  PASS
vehicle-router-smoke reverse set  PASS
```

Phase 2.0 Binder/AIDL runtime gate is also closed. Raspberry Pi runtime on 2026-07-08 verified:

```text
ARDA_VM_GRANT_OK ... source=10000 priority=100 publish=[2, 3] commands=[16]
ARDA_VM_STATUS ... grants=1 ... granted=true signer_ok=true connected=true ready=true
ARDA_VM_RESCAN user=10
ARDA_VM_BIND_REQUEST ... source=10000
ARDA_VM_ADAPTER_READY ... source=10000
SMOKE_ADAPTER_HOST_CONNECTED
ARDA_VM_SIGNAL_ACCEPT ... property=291504903 area=0 type=4
ARDA_VM_SIGNAL_ACCEPT ... property=358614274 area=1 type=4
ARDA_PHASE2_0_RUNTIME_GATE=PASS
```

Phase 2.1 source now implements the manager-to-router protocol bridge. Real AOSP/Soong build and
Raspberry Pi enforcing-SELinux runtime validation remain open for this Phase 2.1 checkpoint.

## Boundary

```text
third-party adapter APK
        |
        | Binder/AIDL
        v
ArdaVehicleManager
        |
        | arda.vehicle.router.v2 over LocalSocket
        v
vehicle-routerd
        |
        v
Wasm policy -> VHAL
```

The adapter AIDL deliberately has no `source_id` field. The manager owns:

```text
source_id
priority
granted publish capabilities
granted command capabilities
signing identity continuity
```

An adapter publishes Android `VehicleProperty` IDs directly:

```text
propertyId
areaId
valueType
typed value
timestampNanos
```

There is no second ARDA signal-ID namespace.

## AIDL API

AIDL module:

```text
arda.vehicle.adapter
```

Current API version:

```text
1
```

Interfaces:

```text
IArdaVehicleAdapter
IArdaVehicleHost
```

Structured parcelables:

```text
AdapterDescriptor
VehicleValue
VehicleCommand
CommandAck
```

`IArdaVehicleAdapter` exposes:

```text
connect(host)
disconnect()
onVehicleCommands(commands)
```

The host callback exposes:

```text
registerAdapter(descriptor)
publishSignals(signals)
acknowledgeCommands(acks)
```

The manager never makes a synchronous Binder call into untrusted adapter code. `connect`,
`disconnect` and command delivery are one-way. Descriptor registration is adapter -> manager; the
adapter blocks on the trusted manager's lightweight validation rather than the manager blocking on
the adapter.

## Discovery and Binder trust boundary

Adapter services export:

```text
com.arda.vehicle.ADAPTER
```

Required service metadata:

```text
com.arda.vehicle.ADAPTER_ID
com.arda.vehicle.DISPLAY_NAME
com.arda.vehicle.API_VERSION
com.arda.vehicle.REQUESTED_PRIORITY
com.arda.vehicle.PUBLISH_CAPABILITIES
com.arda.vehicle.COMMAND_CAPABILITIES
```

Capability metadata accepts names or integer IDs.

The exported adapter service must also require:

```text
com.arda.vehicle.permission.BIND_ADAPTER
```

`ArdaVehicleManager` defines that permission as `signature` and discovery rejects adapter services
that do not protect the Binder entry point with exactly this permission. This prevents an arbitrary
app from binding to an adapter and supplying a fake `IArdaVehicleHost`.

Manifest metadata and the Binder `AdapterDescriptor` registered through the host callback must match.
A mismatch fails the handshake.

## Trust and grants

Discovery does not imply trust.

A grant is scoped by:

```text
foreground Android user
exact service ComponentName
signing identity
manager-assigned source_id
manager-assigned priority
publish capability subset
command capability subset
```

For a normal single-signer package, the grant stores the original signing-certificate digest. A later
certificate is accepted only when Android's verified signing history still contains the stored digest.

For a package with multiple concurrent signers, the complete sorted current signer set must match
exactly.

The manager never accepts an adapter-provided `source_id`.

Grant/revoke/source-ID persistence uses synchronous storage commits and fails closed on persistence
failure. A changed signer, adapter metadata, priority, capability grant or source configuration
replaces the active binding rather than silently reusing stale runtime state.

## Phase 2.1 router bridge

The platform-internal protocol remains `arda.vehicle.router.v2`, protocol version `2`.

Wire framing is unchanged:

```text
4-byte big-endian frame length
protobuf Envelope
```

The Android Java copy of `arda_vehicle_router.proto` is byte-for-byte checked against the Rust/root
protocol source by `scripts/apply-arda-vehicle-phase2-aosp.sh`. The Java module uses protobuf-lite.

The bridge connects to:

```text
/dev/socket/arda_vehicle_router
```

using Android's reserved local-socket namespace.

### One router connection per granted source

Phase 2.1 deliberately does **not** multiplex all adapter sources over one manager socket.

`arda.vehicle.router.v2` has `RegisterSource` but no `UnregisterSource`. Existing router lifecycle is
connection-scoped: when one client disconnects, `cleanup_client()` releases all sources owned by that
client and fails that client's pending commands.

Therefore each ready adapter/source gets its own `ROLE_MANAGER` LocalSocket:

```text
adapter source 10000 -> manager SourceLink 10000 -> router client A
adapter source 10001 -> manager SourceLink 10001 -> router client B
```

A revoke, user switch, adapter rejection or service disconnect closes only the matching source link.
The router's already-proven client cleanup then releases only that source. No protocol-v2 change is
required.

### Connect and registration

For each active source:

```text
Hello
  protocol_version = 2
  role = ROLE_MANAGER

HelloAck

RegisterSource
  source_id = stored manager grant source ID
  source_name = adapter ID + Android user
  priority = stored grant priority
  publish_capabilities = stored grant subset
  command_capabilities = stored grant subset

RegisterSourceResult
```

Handshake reads have a finite timeout. After registration the stream returns to blocking command
receive mode. A failed router connection retries every second; the first failure and each tenth
failure are logged.

### Adapter signals -> router

```text
IArdaVehicleHost.publishSignals(VehicleValue[])
        |
        | caller UID + descriptor/readiness + value validation
        v
AdapterRegistry
        v
RouterBridge SourceLink
        v
VehicleSignals
        v
vehicle-routerd
```

`source_id` is injected from the stored grant. The adapter cannot provide or override it.

When the router is temporarily unavailable, the bridge retains only the latest signal for each
`(propertyId, areaId)` pair, up to 512 distinct keys. This is bounded latest-state retention, not an
unbounded replay queue. Pending latest values are flushed in batches of 128 after reconnect.

`vehicle-routerd` remains authoritative for property/type/area/capability validation. The manager does
not duplicate Rust `SUPPORTED_PROPERTY_RULES`.

### Router commands -> adapter -> ACK

```text
VHAL SET
  -> vehicle-routerd
  -> VehicleCommands(source_id, command_id, VehicleProperty value)
  -> matching per-source Manager SourceLink
  -> IArdaVehicleAdapter.onVehicleCommands()
  -> adapter hardware/OEM/CAN action
  -> IArdaVehicleHost.acknowledgeCommands()
  -> CommandAcks
  -> vehicle-routerd
```

The manager validates command source identity, command ID, value type, finite float values, byte-size
bound and timestamp before crossing the Binder boundary.

If command delivery cannot be queued to a ready adapter, the bridge attempts `COMMAND_IO_ERROR` for
the affected command IDs.

ACKs are **not** queued across router disconnect/reconnect. Router pending-command ownership belongs
to the old connection; replaying an ACK on a new connection would be stale and is intentionally
dropped.

The adapter is still responsible for publishing the hardware-confirmed property state after command
execution. `CommandAck` clears command completion; it is not treated as a synthetic VHAL state event.

## Race hardening

Phase 2.1 also closes several connection-lifecycle races:

- The registry publishes a new `AdapterConnection` as current before calling `bindServiceAsUser`, so
  correctness does not depend on service callback timing.
- `onAdapterReady` verifies the callback belongs to the current connection and holds the registry lock
  through router-source activation. A concurrent revoke/user switch cannot deactivate before the
  source link exists and leave a stale router source behind.
- A rejection from an old/replaced connection cannot remove a newer connection with the same grant
  key.
- Closed source links reject later signal/ACK work.

## Canonical AOSP layout

The canonical product-side source path is:

```text
vendor/arda/vehicle
```

not `packages/apps/ArdaVehicle`.

Production product integration:

```make
$(call inherit-product-if-exists, vendor/arda/vehicle/arda_vehicle.mk)
```

`arda_vehicle.mk` includes only:

```text
ArdaVehicleManager
```

The development smoke adapter is opt-in through:

```text
vendor/arda/vehicle/arda_vehicle_smoke.mk
```

This keeps `ArdaVehicleSmokeAdapter` out of a normal product image.

## Apply and build

From the `aaos-aap` repository:

```bash
AOSP_ROOT=/home/edo/aosp-rpi-aaos16 \
PRODUCT_MK=device/brcm/rpi4/aosp_rpi4_car.mk \
  ./scripts/apply-arda-vehicle-phase2-aosp.sh
```

The apply script:

```text
checks root/Android router proto byte identity
copies source to vendor/arda/vehicle
adds the idempotent product inherit block when PRODUCT_MK is supplied
keeps the smoke adapter opt-in
```

Build:

```bash
source build/envsetup.sh
lunch aosp_rpi4_car-bp4a-userdebug
m ArdaVehicleManager ArdaVehicleSmokeAdapter
```

## Phase 2.1 validation order

1. Run the real AOSP/Soong build above. This is authoritative for generated Java protobuf APIs and
   partition dependency rules.
2. Rebuild `arda-senderd` with the existing Android Docker build path; the local artifact-preparation
   environment did not have Cargo and therefore does not claim a Rust compile pass.
3. Push the new APK/binary development artifacts to the Raspberry Pi.
4. Keep the already-proven smoke adapter grant and require manager status to show:

   ```text
   granted=true signer_ok=true connected=true ready=true
   ```

5. Run:

   ```bash
   DEVICE=192.168.1.21:5555 \
     ./scripts/run-vehicle-manager-bridge-smoke-android.sh
   ```

6. Require router registration:

   ```text
   MANAGER_BRIDGE_ROUTER_REGISTRATION=PASS
   ```

7. Require the Rust end-to-end smoke markers:

   ```text
   VEHICLE_MANAGER_BRIDGE_SMOKE_SET_HANDLED=YES
   VEHICLE_MANAGER_BRIDGE_SMOKE_CONFIRMED_STATE=YES
   VEHICLE_MANAGER_BRIDGE_SMOKE_ACK_ROUND_TRIP=YES
   VEHICLE_MANAGER_BRIDGE_SMOKE_OK=YES
   ```

8. Require:

   ```text
   MANAGER_BRIDGE_ENFORCING_RUNTIME_GATE=PASS
   ```

The smoke runner does **not** switch SELinux to permissive. If manager registration fails, it prints
manager status, router logs, socket/process security contexts and recent AVC denials.

## SELinux/init status

Phase 2.1 source does not claim production daemon packaging is complete.

`vehicle-routerd` still needs final init/product/SELinux integration with a dedicated daemon/socket
security model. A router launched manually from an ADB/root shell may create a socket whose process or
socket label is not connectable by the Manager domain under enforcing SELinux.

If the Phase 2.1 enforcing runtime gate reports AVC denials, the next step is **not** to hide the issue
with `setenforce 0`. The next step is to package `vehicle-routerd` as an init-managed service and add
the narrow dedicated SELinux policy required for the Manager and VHAL clients.

### Phase 2.1 LocalSocket option ordering

`android.net.LocalSocket#setSoTimeout()` does not create the underlying AF_UNIX socket file descriptor. The manager must call `connect()` first and only then set the handshake receive/send timeout. Calling `setSoTimeout()` on a fresh `LocalSocket` fails locally with `IOException: socket not created` before any router connection or SELinux check occurs.

## Phase 2.2: init-managed router and dedicated SELinux boundary

The first real enforcing Phase 2.1 connection proved that a manually root-launched router leaves the
socket as generic `socket_device` and a Manager running in generic `system_app` is denied `sock_file
write` before UNIX stream connect. The production fix is a dedicated platform boundary, not a broad
allow rule for all system apps.

### Init-owned listening socket

The system_ext init service is:

```text
service arda_vehicle_router /system_ext/bin/arda-senderd vehicle-routerd
    class main
    user root
    group root system
    socket arda_vehicle_router stream+listen 0660 root system u:object_r:arda_vehicle_router_socket:s0
    stdio_to_kmsg
```

Init creates, binds and listens on `/dev/socket/arda_vehicle_router` before starting the service and
passes the listener through `ANDROID_SOCKET_arda_vehicle_router`.

For the default router path, Rust prefers the inherited init descriptor. Before taking ownership it
verifies:

```text
fd >= 0
fcntl(F_GETFD) succeeds
SO_TYPE == SOCK_STREAM
SO_ACCEPTCONN == 1
local pathname == /dev/socket/arda_vehicle_router
```

Successful startup logs:

```text
VEHICLE_ROUTER_SOCKET_MODE=init_inherited ... listening=true
VEHICLE_ROUTER_READY ... socket_mode=init_inherited
```

When the init socket environment variable is absent, the previous manual bind/chmod/chown path remains
available. A custom `--socket` path intentionally ignores the default init descriptor. This preserves
existing isolated router smokes.

### Dedicated domains

The router executable transitions from init into `arda_vehicle_router`. The init-created socket is
`arda_vehicle_router_socket`, a core-domain socket type.

The platform-signed privileged Manager receives a package-specific `arda_vehicle_manager` app domain
through system_ext private `seapp_contexts`. Only that domain receives:

```text
allow arda_vehicle_manager arda_vehicle_router_socket:sock_file write;
allow arda_vehicle_manager arda_vehicle_router_socket:unix_stream_socket connectto;
```

There is no generic `allow system_app socket_device:sock_file write` rule.

On userdebug/eng builds only, `su` may connect to the router so the existing root VHAL smoke client in
`/data/local/tmp/arda-senderd` can run. This debug allowance is not part of a user build.

### Build integration

After the Android Docker Rust build:

```bash
./scripts/stage-arda-vehicle-router-prebuilt.sh
```

The staging script requires a non-empty executable Android ARM64 ELF using `/system/bin/linker64` and
copies it to:

```text
$ARDA_VEHICLE_SOURCE/router-service/prebuilt/arda-senderd
```

The generated prebuilt is ignored by Git in `android_vendor_arda/vehicle` but is copied into the frozen AOSP source tree for Soong.

`apply-arda-vehicle-phase2-aosp.sh` now requires the staged prebuilt before copy and can integrate:

```text
PRODUCT_MK -> vendor/arda/vehicle/arda_vehicle.mk
BOARD_CONFIG -> SYSTEM_EXT_PRIVATE_SEPOLICY_DIRS += vendor/arda/vehicle/sepolicy/private
```

Both edits are idempotent and normalized to one canonical entry.

### Enforcing runtime gate

The Phase 2.1 smoke runner no longer launches or deletes a router socket. It requires the init service
to be `running`, then requires:

```text
/dev/socket/arda_vehicle_router -> u:object_r:arda_vehicle_router_socket:s0
arda-senderd                    -> u:r:arda_vehicle_router:s0
com.arda.vehicle.manager        -> u:r:arda_vehicle_manager:s0
```

Only then does it wait for Manager `REGISTERED`, run the VHAL-only command/ACK/confirmed-state smoke,
and require:

```text
VEHICLE_MANAGER_BRIDGE_SMOKE_SET_HANDLED=YES
VEHICLE_MANAGER_BRIDGE_SMOKE_CONFIRMED_STATE=YES
VEHICLE_MANAGER_BRIDGE_SMOKE_ACK_ROUND_TRIP=YES
VEHICLE_MANAGER_BRIDGE_SMOKE_OK=YES
MANAGER_BRIDGE_ENFORCING_RUNTIME_GATE=PASS selinux=Enforcing
```

Phase 2.2 must remain open until the real AOSP SELinux build and Raspberry Pi enforcing gate pass.

### Phase 2.1 / 2.2 closure

The Enforcing Raspberry Pi runtime gate passed on 2026-07-08. The Manager registered source `10000`,
the VHAL smoke dispatched `HVAC_TEMPERATURE_SET`, the smoke adapter returned `CommandAck OK`, and the
adapter-confirmed state returned as a VHAL event.

```text
VEHICLE_MANAGER_BRIDGE_SMOKE_SET_HANDLED=YES
VEHICLE_MANAGER_BRIDGE_SMOKE_CONFIRMED_STATE=YES
VEHICLE_MANAGER_BRIDGE_SMOKE_ACK_ROUND_TRIP=YES
VEHICLE_MANAGER_BRIDGE_SMOKE_OK=YES
MANAGER_BRIDGE_ENFORCING_RUNTIME_GATE=PASS selinux=Enforcing
```

Canonical Android source is now `Deadknight/android_vendor_arda` under `vehicle/`; the AOSP destination
remains `vendor/arda/vehicle`.
