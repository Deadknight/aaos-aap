use std::fs;
use std::path::Path;

use wasmtime::{
    Caller, Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder, TypedFunc,
};

use super::model::{
    validate_vehicle_command, validate_vhal_event, validate_vhal_set_request, VehicleCommand,
    VehicleSignal, VehicleValue, VhalEvent, VhalSetRequest,
};

pub const VEHICLE_WASM_ABI_VERSION: i32 = 2;
const BUILTIN_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/arda_vhal_default.wasm"));
const MAX_WASM_MODULE_BYTES: usize = 4 * 1024 * 1024;
const MAX_WASM_LINEAR_MEMORY_BYTES: usize = 2 * 1024 * 1024;
const MAX_WASM_TABLE_ELEMENTS: usize = 1024;

#[derive(Debug)]
struct WasmHostState {
    limits: StoreLimits,
    vhal_events: Vec<VhalEvent>,
    vehicle_commands: Vec<VehicleCommand>,
    max_outputs: usize,
    dropped_outputs: u64,
}

impl WasmHostState {
    fn new(max_outputs: usize) -> Self {
        Self {
            limits: StoreLimitsBuilder::new()
                .memory_size(MAX_WASM_LINEAR_MEMORY_BYTES)
                .table_elements(MAX_WASM_TABLE_ELEMENTS)
                .instances(2)
                .tables(1)
                .memories(1)
                .trap_on_grow_failure(true)
                .build(),
            vhal_events: Vec::new(),
            vehicle_commands: Vec::new(),
            max_outputs: max_outputs.max(1),
            dropped_outputs: 0,
        }
    }

    fn output_count(&self) -> usize {
        self.vhal_events.len() + self.vehicle_commands.len()
    }

    fn push_vhal(&mut self, event: VhalEvent) -> i32 {
        if self.output_count() >= self.max_outputs {
            self.dropped_outputs = self.dropped_outputs.saturating_add(1);
            return -2;
        }
        self.vhal_events.push(event);
        0
    }

    fn push_vehicle(&mut self, command: VehicleCommand) -> i32 {
        if self.output_count() >= self.max_outputs {
            self.dropped_outputs = self.dropped_outputs.saturating_add(1);
            return -2;
        }
        self.vehicle_commands.push(command);
        0
    }

    fn reset(&mut self) {
        self.vhal_events.clear();
        self.vehicle_commands.clear();
        self.dropped_outputs = 0;
    }
}

#[derive(Debug)]
pub struct VehiclePolicyOutput {
    pub status: i32,
    pub vhal_events: Vec<VhalEvent>,
    pub vehicle_commands: Vec<VehicleCommand>,
    pub dropped_outputs: u64,
}

pub struct VehiclePolicyEngine {
    store: Store<WasmHostState>,
    on_vehicle_signal: TypedFunc<(i32, i32, i32, i32, i64, f64), i32>,
    on_vhal_set: TypedFunc<(i32, i32, i32, i64, f64), i32>,
    reset_capability: TypedFunc<(i32, i32), i32>,
    fuel_per_call: u64,
    pub module_label: String,
}

impl VehiclePolicyEngine {
    pub fn new(
        wasm_spec: &str,
        fuel_per_call: u64,
        max_outputs: usize,
    ) -> Result<(Self, i32), Box<dyn std::error::Error>> {
        let (module_label, module_bytes) = load_module_bytes(wasm_spec)?;

        let mut engine_config = Config::new();
        engine_config.consume_fuel(true);
        let engine = Engine::new(&engine_config)?;
        let module = Module::from_binary(&engine, &module_bytes)?;
        let mut linker = Linker::new(&engine);

        link_vhal_emitters(&mut linker)?;
        link_vehicle_emitters(&mut linker)?;

        let mut store = Store::new(&engine, WasmHostState::new(max_outputs));
        store.limiter(|state| &mut state.limits);
        store.set_fuel(fuel_per_call.max(1))?;
        let instance = linker.instantiate(&mut store, &module)?;

        let abi_version = instance
            .get_typed_func::<(), i32>(&mut store, "arda_abi_version")?
            .call(&mut store, ())?;
        if abi_version != VEHICLE_WASM_ABI_VERSION {
            return Err(format!(
                "vehicle wasm ABI mismatch: module={} expected={} actual={}",
                module_label, VEHICLE_WASM_ABI_VERSION, abi_version
            )
            .into());
        }

        let on_vehicle_signal = instance.get_typed_func::<
            (i32, i32, i32, i32, i64, f64),
            i32,
        >(&mut store, "arda_on_vehicle_signal")?;
        let on_vhal_set = instance.get_typed_func::<(i32, i32, i32, i64, f64), i32>(
            &mut store,
            "arda_on_vhal_set",
        )?;
        let reset_capability = instance.get_typed_func::<(i32, i32), i32>(
            &mut store,
            "arda_reset_capability",
        )?;

        Ok((
            Self {
                store,
                on_vehicle_signal,
                on_vhal_set,
                reset_capability,
                fuel_per_call: fuel_per_call.max(1),
                module_label,
            },
            abi_version,
        ))
    }

    pub fn process_vehicle_signal(
        &mut self,
        signal: &VehicleSignal,
    ) -> Result<VehiclePolicyOutput, Box<dyn std::error::Error>> {
        super::model::validate_vehicle_signal(signal)
            .map_err(|reason| format!("vehicle signal rejected before wasm: {reason}"))?;
        self.reset_call()?;

        let status = match self.on_vehicle_signal.call(
            &mut self.store,
            (
                i32::try_from(signal.source_id)
                    .expect("validated vehicle source_id must fit Wasm i32 lane"),
                signal.property_id,
                signal.area_id,
                signal.value.value_type_id(),
                signal.value.i64_lane(),
                signal.value.f64_lane(),
            ),
        ) {
            Ok(status) => status,
            Err(err) => {
                self.store.data_mut().reset();
                return Err(err.into());
            }
        };

        self.finish_call(status, signal.timestamp_nanos, 0)
    }

    pub fn reset_capability(
        &mut self,
        capability_id: i32,
        area_id: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.reset_call()?;
        let status = match self
            .reset_capability
            .call(&mut self.store, (capability_id, area_id))
        {
            Ok(status) => status,
            Err(err) => {
                self.store.data_mut().reset();
                return Err(err.into());
            }
        };
        self.store.data_mut().reset();
        if status != 0 {
            return Err(format!(
                "vehicle wasm capability reset rejected capability={} area={} status={}",
                capability_id, area_id, status
            )
            .into());
        }
        Ok(())
    }

    pub fn process_vhal_set(
        &mut self,
        request: &VhalSetRequest,
    ) -> Result<VehiclePolicyOutput, Box<dyn std::error::Error>> {
        validate_vhal_set_request(request)
            .map_err(|reason| format!("vhal set rejected before wasm: {reason}"))?;
        self.reset_call()?;

        let status = match self.on_vhal_set.call(
            &mut self.store,
            (
                request.property_id,
                request.area_id,
                request.value.value_type_id(),
                request.value.i64_lane(),
                request.value.f64_lane(),
            ),
        ) {
            Ok(status) => status,
            Err(err) => {
                self.store.data_mut().reset();
                return Err(err.into());
            }
        };

        self.finish_call(status, request.timestamp_nanos, request.request_id)
    }

    fn reset_call(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.store.data_mut().reset();
        self.store.set_fuel(self.fuel_per_call)?;
        Ok(())
    }

    fn finish_call(
        &mut self,
        status: i32,
        timestamp_nanos: i64,
        command_id: u64,
    ) -> Result<VehiclePolicyOutput, Box<dyn std::error::Error>> {
        let state = self.store.data_mut();
        let mut vhal_events = std::mem::take(&mut state.vhal_events);
        let mut vehicle_commands = std::mem::take(&mut state.vehicle_commands);
        let dropped_outputs = std::mem::take(&mut state.dropped_outputs);

        if status != 0 {
            vhal_events.clear();
            vehicle_commands.clear();
        }

        for event in &mut vhal_events {
            event.timestamp_nanos = timestamp_nanos;
            validate_vhal_event(event)
                .map_err(|reason| format!("wasm emitted invalid VHAL event: {reason}"))?;
        }
        for command in &mut vehicle_commands {
            command.command_id = command_id;
            command.timestamp_nanos = timestamp_nanos;
            validate_vehicle_command(command)
                .map_err(|reason| format!("wasm emitted invalid vehicle command: {reason}"))?;
        }

        Ok(VehiclePolicyOutput {
            status,
            vhal_events,
            vehicle_commands,
            dropped_outputs,
        })
    }
}

fn link_vhal_emitters(linker: &mut Linker<WasmHostState>) -> Result<(), Box<dyn std::error::Error>> {
    linker.func_wrap(
        "arda_vhal",
        "emit_bool",
        |mut caller: Caller<'_, WasmHostState>, property_id: i32, area_id: i32, value: i32| -> i32 {
            caller.data_mut().push_vhal(VhalEvent {
                property_id,
                area_id,
                value: VehicleValue::Bool(value != 0),
                timestamp_nanos: 0,
            })
        },
    )?;
    linker.func_wrap(
        "arda_vhal",
        "emit_i32",
        |mut caller: Caller<'_, WasmHostState>, property_id: i32, area_id: i32, value: i32| -> i32 {
            caller.data_mut().push_vhal(VhalEvent {
                property_id,
                area_id,
                value: VehicleValue::I32(value),
                timestamp_nanos: 0,
            })
        },
    )?;
    linker.func_wrap(
        "arda_vhal",
        "emit_i64",
        |mut caller: Caller<'_, WasmHostState>, property_id: i32, area_id: i32, value: i64| -> i32 {
            caller.data_mut().push_vhal(VhalEvent {
                property_id,
                area_id,
                value: VehicleValue::I64(value),
                timestamp_nanos: 0,
            })
        },
    )?;
    linker.func_wrap(
        "arda_vhal",
        "emit_f32",
        |mut caller: Caller<'_, WasmHostState>, property_id: i32, area_id: i32, value: f32| -> i32 {
            caller.data_mut().push_vhal(VhalEvent {
                property_id,
                area_id,
                value: VehicleValue::F32(value),
                timestamp_nanos: 0,
            })
        },
    )?;
    Ok(())
}

fn link_vehicle_emitters(
    linker: &mut Linker<WasmHostState>,
) -> Result<(), Box<dyn std::error::Error>> {
    linker.func_wrap(
        "arda_vehicle",
        "emit_bool",
        |mut caller: Caller<'_, WasmHostState>, property_id: i32, area_id: i32, value: i32| -> i32 {
            caller.data_mut().push_vehicle(VehicleCommand {
                command_id: 0,
                source_id: 0,
                property_id,
                area_id,
                value: VehicleValue::Bool(value != 0),
                timestamp_nanos: 0,
            })
        },
    )?;
    linker.func_wrap(
        "arda_vehicle",
        "emit_i32",
        |mut caller: Caller<'_, WasmHostState>, property_id: i32, area_id: i32, value: i32| -> i32 {
            caller.data_mut().push_vehicle(VehicleCommand {
                command_id: 0,
                source_id: 0,
                property_id,
                area_id,
                value: VehicleValue::I32(value),
                timestamp_nanos: 0,
            })
        },
    )?;
    linker.func_wrap(
        "arda_vehicle",
        "emit_i64",
        |mut caller: Caller<'_, WasmHostState>, property_id: i32, area_id: i32, value: i64| -> i32 {
            caller.data_mut().push_vehicle(VehicleCommand {
                command_id: 0,
                source_id: 0,
                property_id,
                area_id,
                value: VehicleValue::I64(value),
                timestamp_nanos: 0,
            })
        },
    )?;
    linker.func_wrap(
        "arda_vehicle",
        "emit_f32",
        |mut caller: Caller<'_, WasmHostState>, property_id: i32, area_id: i32, value: f32| -> i32 {
            caller.data_mut().push_vehicle(VehicleCommand {
                command_id: 0,
                source_id: 0,
                property_id,
                area_id,
                value: VehicleValue::F32(value),
                timestamp_nanos: 0,
            })
        },
    )?;
    Ok(())
}

fn load_module_bytes(wasm_spec: &str) -> Result<(String, Vec<u8>), Box<dyn std::error::Error>> {
    let normalized = wasm_spec.trim();
    if normalized.is_empty()
        || normalized.eq_ignore_ascii_case("builtin")
        || normalized.eq_ignore_ascii_case("default")
        || normalized.eq_ignore_ascii_case("embedded")
    {
        return Ok(("builtin:arda_vhal_default-v2".to_string(), BUILTIN_WASM.to_vec()));
    }

    let path = Path::new(normalized);
    let metadata = fs::metadata(path)
        .map_err(|err| format!("cannot stat vehicle wasm {}: {err}", path.display()))?;
    let module_len = usize::try_from(metadata.len())
        .map_err(|_| format!("vehicle wasm {} is too large for this platform", path.display()))?;
    if module_len > MAX_WASM_MODULE_BYTES {
        return Err(format!(
            "vehicle wasm {} exceeds max module bytes: actual={} max={}",
            path.display(), module_len, MAX_WASM_MODULE_BYTES
        )
        .into());
    }
    let bytes = fs::read(path)
        .map_err(|err| format!("cannot read vehicle wasm {}: {err}", path.display()))?;
    Ok((format!("file:{}", path.display()), bytes))
}

pub fn ready_line(
    engine: &VehiclePolicyEngine,
    abi_version: i32,
    fuel_per_call: u64,
    max_outputs: usize,
) -> String {
    format!(
        "VEHICLE_WASM_ENGINE ready abi={} module={} fuel_per_call={} max_outputs={} memory_limit_bytes={} table_limit_elements={} wasi=false exports=arda_on_vehicle_signal(source_id,...),arda_on_vhal_set(...),arda_reset_capability(...);imports=arda_vhal.emit_bool,emit_i32,emit_i64,emit_f32;arda_vehicle.emit_bool,emit_i32,emit_i64,emit_f32",
        abi_version,
        engine.module_label,
        fuel_per_call.max(1),
        max_outputs.max(1),
        MAX_WASM_LINEAR_MEMORY_BYTES,
        MAX_WASM_TABLE_ELEMENTS,
    )
}
