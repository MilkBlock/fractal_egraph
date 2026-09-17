// Keep the upstream WASM runtime isolated. Native pattern/trace rendering uses
// the local debugger bridge, avoiding multiple global wasm_bindgen declarations.
let logbuffer = [];
function log(level, str) { logbuffer.push([level, str]); }
importScripts("egglog_demo.js");
const ready = wasm_bindgen("egglog_demo_bg.wasm");
self.onmessage = async event => {
    try {
        await ready;
        logbuffer = [];
        const result = wasm_bindgen.run_program(event.data);
        self.postMessage({dot:result.dot, text:result.text, log:logbuffer,
                          json:result.json, omitted:result.omitted});
        result.free();
    } catch (error) {
        self.postMessage({dot:"", text:String(error), log:logbuffer, json:"{}"});
    }
};
