(module
  ;; ARDA vehicle-policy ABI v2.
  ;;
  ;; VehicleSignal.property_id carries the Android VehicleProperty ID directly.
  ;; The default policy is therefore a
  ;; typed pass-through policy, not a property-by-property switch table.
  ;;
  ;; HU/AAP SensorSource mapping is done in Rust before this module. Third-party
  ;; adapter developers may publish standard VehicleProperty IDs directly.
  ;; A custom Wasm module can still filter, remap or add policy when an OEM
  ;; integration actually needs it.

  (import "arda_vhal" "emit_bool"
    (func $vhal_bool (param i32 i32 i32) (result i32)))
  (import "arda_vhal" "emit_i32"
    (func $vhal_i32 (param i32 i32 i32) (result i32)))
  (import "arda_vhal" "emit_i64"
    (func $vhal_i64 (param i32 i32 i64) (result i32)))
  (import "arda_vhal" "emit_f32"
    (func $vhal_f32 (param i32 i32 f32) (result i32)))

  (import "arda_vehicle" "emit_bool"
    (func $vehicle_bool (param i32 i32 i32) (result i32)))
  (import "arda_vehicle" "emit_i32"
    (func $vehicle_i32 (param i32 i32 i32) (result i32)))
  (import "arda_vehicle" "emit_i64"
    (func $vehicle_i64 (param i32 i32 i64) (result i32)))
  (import "arda_vehicle" "emit_f32"
    (func $vehicle_f32 (param i32 i32 f32) (result i32)))

  ;; ValueType IDs from arda_vehicle_router.proto.
  ;; 1 BOOL
  ;; 2 I32
  ;; 3 I64
  ;; 4 F32
  ;; 5 F64   (not exposed by the scalar ABI v2 host imports yet)
  ;; 6 BYTES (requires a future memory/span ABI)

  (func (export "arda_abi_version") (result i32)
    i32.const 2)

  ;; The default policy is stateless, so capability ownership changes require
  ;; no guest-state cleanup. Custom stateful policies may use this hook.
  (func (export "arda_reset_capability")
    (param $capability i32)
    (param $area i32)
    (result i32)
    i32.const 0)

  (func (export "arda_on_vehicle_signal")
    (param $source i32)
    (param $property i32)
    (param $area i32)
    (param $value_type i32)
    (param $i64_value i64)
    (param $f64_value f64)
    (result i32)

    ;; BOOL
    local.get $value_type
    i32.const 1
    i32.eq
    if
      local.get $property
      local.get $area
      local.get $i64_value
      i32.wrap_i64
      call $vhal_bool
      return
    end

    ;; I32
    local.get $value_type
    i32.const 2
    i32.eq
    if
      local.get $property
      local.get $area
      local.get $i64_value
      i32.wrap_i64
      call $vhal_i32
      return
    end

    ;; I64
    local.get $value_type
    i32.const 3
    i32.eq
    if
      local.get $property
      local.get $area
      local.get $i64_value
      call $vhal_i64
      return
    end

    ;; F32
    local.get $value_type
    i32.const 4
    i32.eq
    if
      local.get $property
      local.get $area
      local.get $f64_value
      f32.demote_f64
      call $vhal_f32
      return
    end

    ;; Unsupported scalar/memory representation in ABI v2.
    i32.const 12)

  (func (export "arda_on_vhal_set")
    (param $property i32)
    (param $area i32)
    (param $value_type i32)
    (param $i64_value i64)
    (param $f64_value f64)
    (result i32)

    ;; BOOL
    local.get $value_type
    i32.const 1
    i32.eq
    if
      local.get $property
      local.get $area
      local.get $i64_value
      i32.wrap_i64
      call $vehicle_bool
      return
    end

    ;; I32
    local.get $value_type
    i32.const 2
    i32.eq
    if
      local.get $property
      local.get $area
      local.get $i64_value
      i32.wrap_i64
      call $vehicle_i32
      return
    end

    ;; I64
    local.get $value_type
    i32.const 3
    i32.eq
    if
      local.get $property
      local.get $area
      local.get $i64_value
      call $vehicle_i64
      return
    end

    ;; F32
    local.get $value_type
    i32.const 4
    i32.eq
    if
      local.get $property
      local.get $area
      local.get $f64_value
      f32.demote_f64
      call $vehicle_f32
      return
    end

    i32.const 12)
)
