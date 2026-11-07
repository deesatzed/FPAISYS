//! Fixed synthetic demonstration guests. This registry is not a sandbox or a plugin loader.
pub const NAMES: &[&str] = &["allow", "throttle", "quarantine", "window-probe"];
pub fn artifact(name: &str) -> Result<Vec<u8>, String> {
    let wat = match name {
        "allow" => "(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 local.get 6 local.get 7))",
        "throttle" => "(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 1 local.get 6 local.get 7))",
        "quarantine" => "(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 2 local.get 6 local.get 7))",
        "window-probe" => "(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 local.get 1 local.get 0 f64.convert_i64_u f64.add local.get 2 f64.convert_i64_u f64.add local.get 3))",
        _ => return Err("unknown built-in synthetic candidate".into()),
    };
    wat::parse_str(wat).map_err(|e| e.to_string())
}
