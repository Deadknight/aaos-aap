(module
  (import "arda_vhal" "emit_bool" (func $emit_bool (param i32 i32 i32) (result i32)))
  (import "arda_vhal" "emit_i32" (func $emit_i32 (param i32 i32 i32) (result i32)))
  (import "arda_vhal" "emit_f32" (func $emit_f32 (param i32 i32 f32) (result i32)))

  (func (export "arda_abi_version") (result i32)
    i32.const 1)

  ;; ABI v1:
  ;;   arda_on_sensor(kind, a, b, c, d, e, f) -> status
  ;; Sensor kind values intentionally match AAP SensorType ids.
  ;; Guest code has no WASI imports. Its only authority is requesting typed
  ;; VHAL commands through the three host imports above; the Rust host validates
  ;; property allow-list, type, area, finite/range constraints, and rate limits.
  (func (export "arda_on_sensor")
    (param $kind i32)
    (param $a f64)
    (param $b f64)
    (param $c f64)
    (param $d f64)
    (param $e f64)
    (param $f f64)
    (result i32)

    ;; SENSOR_SPEED(3) -> PERF_VEHICLE_SPEED + DISPLAY
    local.get $kind
    i32.const 3
    i32.eq
    if
      i32.const 291504647
      i32.const 0
      local.get $a
      f32.demote_f64
      call $emit_f32
      drop

      i32.const 291504648
      i32.const 0
      local.get $a
      f32.demote_f64
      call $emit_f32
      drop
    end

    ;; SENSOR_RPM(4) -> ENGINE_RPM
    local.get $kind
    i32.const 4
    i32.eq
    if
      i32.const 291504901
      i32.const 0
      local.get $a
      f32.demote_f64
      call $emit_f32
      drop
    end

    ;; SENSOR_ODOMETER(5) -> PERF_ODOMETER
    local.get $kind
    i32.const 5
    i32.eq
    if
      i32.const 291504644
      i32.const 0
      local.get $a
      f32.demote_f64
      call $emit_f32
      drop
    end

    ;; SENSOR_FUEL(6) is normalized but intentionally not mapped by the
    ;; built-in policy: the current AAP proto does not define fuel/range units.
    ;; An OEM-specific external module may map it once source units are known.

    ;; SENSOR_PARKING_BRAKE(7)
    local.get $kind
    i32.const 7
    i32.eq
    if
      i32.const 287310850
      i32.const 0
      local.get $a
      i32.trunc_sat_f64_s
      call $emit_bool
      drop
    end

    ;; SENSOR_GEAR(8): b is translated android.car.VehicleGear, c says mapped.
    local.get $kind
    i32.const 8
    i32.eq
    if
      local.get $c
      f64.const 0
      f64.gt
      if
        i32.const 289408001
        i32.const 0
        local.get $b
        i32.trunc_sat_f64_s
        call $emit_i32
        drop

        i32.const 289408000
        i32.const 0
        local.get $b
        i32.trunc_sat_f64_s
        call $emit_i32
        drop
      end
    end

    ;; SENSOR_NIGHT_MODE(10)
    local.get $kind
    i32.const 10
    i32.eq
    if
      i32.const 287310855
      i32.const 0
      local.get $a
      i32.trunc_sat_f64_s
      call $emit_bool
      drop
    end

    ;; SENSOR_ENVIRONMENT_DATA(11) -> ENV_OUTSIDE_TEMPERATURE
    local.get $kind
    i32.const 11
    i32.eq
    if
      i32.const 291505923
      i32.const 0
      local.get $a
      f32.demote_f64
      call $emit_f32
      drop
    end

    i32.const 0)
)
