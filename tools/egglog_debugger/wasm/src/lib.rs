//! Browser entry points for the native debugger.
//!
//! This is the same patched, instrumented egglog runtime the local bridge runs, compiled
//! to wasm32, so the published page needs no local process. The analysis rule programs
//! are embedded (`egg_layout::embedded_rules`) and the browser supplies the .egg text.
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

fn js_error(error: impl std::fmt::Display) -> JsError {
    JsError::new(&error.to_string())
}

/// Native `debug-patterns`: parse source ranges plus the math-view/DOT rows, as JSON.
#[wasm_bindgen]
pub fn debug_patterns(source: &str) -> Result<String, JsError> {
    egg_layout::native_analyze::debug::patterns(source)
        .map(|value| value.to_string())
        .map_err(js_error)
}

/// Native `debug-stream`: run the instrumented runtime and hand each JSON row to `emit`.
#[wasm_bindgen]
pub fn debug_stream(source: &str, emit: &js_sys::Function) -> Result<(), JsError> {
    let mut callback_error: Option<JsValue> = None;
    let result = egg_layout::native_analyze::debug::stream_source(
        std::path::Path::new(""),
        "input.egg",
        source,
        &mut |row| {
            if callback_error.is_none() {
                let value = JsValue::from_str(&row.to_string());
                if let Err(error) = emit.call1(&JsValue::NULL, &value) {
                    callback_error = Some(error);
                }
            }
            Ok(())
        },
    );
    if let Some(error) = callback_error {
        return Err(JsError::new(&format!("emit callback failed: {error:?}")));
    }
    result.map_err(js_error)
}
