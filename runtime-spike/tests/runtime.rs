use sese_runtime_spike::evaluate;

fn wasm(source: &str) -> Vec<u8> {
    wat::parse_str(source).unwrap()
}

#[test]
fn evaluates_a_real_function() {
    let code = wasm("(module (func (export \"evaluate\") (param i32) (result i32) local.get 0 i32.const 1 i32.add))");
    assert_eq!(evaluate(&code, 41).unwrap(), 42);
}

#[test]
fn rejects_host_imports() {
    let code = wasm("(module (import \"host\" \"io\" (func)) (func (export \"evaluate\") (param i32) (result i32) i32.const 0))");
    assert!(evaluate(&code, 0).is_err());
}

#[test]
fn stops_infinite_loop() {
    let code = wasm(
        "(module (func (export \"evaluate\") (param i32) (result i32) (loop br 0) i32.const 0))",
    );
    assert!(evaluate(&code, 0).is_err());
}

#[test]
fn rejects_excessive_initial_memory() {
    let code = wasm(
        "(module (memory 20) (func (export \"evaluate\") (param i32) (result i32) i32.const 0))",
    );
    assert!(evaluate(&code, 0).is_err());
}

#[test]
fn fresh_instance_does_not_retain_guest_state() {
    let code = wasm("(module (global $g (mut i32) (i32.const 0)) (func (export \"evaluate\") (param i32) (result i32) global.get $g i32.const 1 i32.add global.set $g global.get $g))");
    assert_eq!(evaluate(&code, 0).unwrap(), 1);
    assert_eq!(evaluate(&code, 0).unwrap(), 1);
}

#[test]
fn memory_growth_is_limited() {
    let code = wasm("(module (memory 1) (func (export \"evaluate\") (param i32) (result i32) i32.const 10 memory.grow))");
    assert!(
        evaluate(&code, 0).is_err(),
        "denied memory growth must trap"
    );
}

#[test]
fn rejects_malformed_and_oversized_modules() {
    assert!(evaluate(b"not wasm", 0).is_err());
    assert!(evaluate(&vec![0; 65_537], 0).is_err());
}

#[test]
fn window_abi_passes_snapshot_and_event_to_real_guest() {
    let code=wasm("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) local.get 0 local.get 1 i32.wrap_i64 i32.add local.get 2 i32.add))");
    assert_eq!(
        sese_runtime_spike::evaluate_window(&code, &[10, 20], 3).unwrap(),
        35
    );
    assert_eq!(
        sese_runtime_spike::evaluate_window(&code, &[], 3).unwrap(),
        3
    );
}
#[test]
fn window_abi_rejects_excessive_window_and_wrong_signature() {
    let code = wasm("(module (func (export \"evaluate\") (param i32) (result i32) local.get 0))");
    assert!(sese_runtime_spike::evaluate_window(&code, &[], 0).is_err());
    assert!(sese_runtime_spike::evaluate_window(&code, &vec![0; 4097], 0).is_err());
}

#[test]
fn window_abi_applies_fuel_and_import_restrictions() {
    let loop_code=wasm("(module (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) (loop br 0) i32.const 0))");
    let import_code=wasm("(module (import \"host\" \"io\" (func)) (func (export \"evaluate_window_v1\") (param i32 i64 i32) (result i32) i32.const 0))");
    assert!(sese_runtime_spike::evaluate_window(&loop_code, &[1], 0).is_err());
    assert!(sese_runtime_spike::evaluate_window(&import_code, &[1], 0).is_err());
}

#[test]
fn synthetic_modules_exceeding_parser_structure_limits_are_rejected() {
    fn module(globals: usize) -> Vec<u8> {
        let mut wat = String::from("(module ");
        for _ in 0..globals {
            wat.push_str("(global i32 (i32.const 0)) ");
        }
        wat.push_str("(func (export \"evaluate\") (param i32) (result i32) local.get 0))");
        wat::parse_str(wat).unwrap()
    }
    let at_limit = module(1000);
    let over_limit = module(1001);
    assert!(over_limit.len() < 65_536);
    assert_eq!(sese_runtime_spike::evaluate(&at_limit, 7).unwrap(), 7);
    assert!(
        sese_runtime_spike::evaluate(&over_limit, 7).is_err(),
        "global count must be bounded during module parsing"
    );
}

#[test]
fn parser_bounds_function_signature_size_before_execution() {
    fn module(params: usize) -> Vec<u8> {
        let arguments = " i32".repeat(params);
        wat::parse_str(format!("(module (func (param{arguments})) (func (export \"evaluate\") (param i32) (result i32) local.get 0))")).unwrap()
    }
    assert_eq!(sese_runtime_spike::evaluate(&module(32), 7).unwrap(), 7);
    let error = sese_runtime_spike::evaluate(&module(33), 7).unwrap_err();
    assert!(error.contains("32 parameters"), "{error}");
}

#[test]
fn resource_growth_within_limits_succeeds_and_denied_table_growth_traps() {
    let memory=wasm("(module (memory 1) (func (export \"evaluate\") (param i32) (result i32) i32.const 3 memory.grow))");
    assert_eq!(evaluate(&memory, 0).unwrap(), 1);
    let table=wasm("(module (table 1 funcref) (func (export \"evaluate\") (param i32) (result i32) ref.null func i32.const 2048 table.grow drop i32.const 0))");
    assert!(
        evaluate(&table, 0).is_err(),
        "denied table growth must trap even when the guest ignores the result"
    );
}

#[test]
fn impossible_memory_growth_traps_even_if_guest_ignores_result() {
    let code=wasm("(module (memory 1) (func (export \"evaluate\") (param i32) (result i32) i32.const -1 memory.grow drop i32.const 7))");
    assert!(
        evaluate(&code, 0).is_err(),
        "invalid growth must not be ignored"
    );
}

#[test]
fn impossible_memory_growth_without_a_limiter_keeps_wasm_behavior() {
    let code = wasm(
        "(module (memory 1) (func (export \"evaluate\") (result i32) i32.const -1 memory.grow))",
    );
    let engine = wasmi::Engine::default();
    let module = wasmi::Module::new(&engine, &code[..]).unwrap();
    let mut store = wasmi::Store::new(&engine, ());
    let instance = wasmi::Linker::new(&engine)
        .instantiate(&mut store, &module)
        .unwrap()
        .start(&mut store)
        .unwrap();
    let function = instance
        .get_typed_func::<(), i32>(&store, "evaluate")
        .unwrap();
    assert_eq!(function.call(&mut store, ()).unwrap(), -1);
}
