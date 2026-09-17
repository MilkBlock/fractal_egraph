// Run the instrumented runtime off the main thread.
//
// `debug_stream` is a synchronous wasm call, so running it on the page froze the
// UI until the whole program finished: the incremental log then appeared all at
// once. In a worker every row can be posted as it is produced, and `stop` becomes
// a real `terminate()`.
import init, { debug_stream } from "./wasm/egglog_debug_wasm.js";

let ready = null;
const ensure = () => (ready ||= init());

// Posting one message per row costs more than parsing it; batch a little so the
// page still paints continuously without a message storm.
const BATCH = 64;

self.onmessage = async event => {
    const { source } = event.data ?? {};
    if (typeof source !== "string") return;
    try {
        await ensure();
        let batch = [];
        const flush = () => { if (batch.length) { self.postMessage({ rows: batch }); batch = []; } };
        debug_stream(source, line => {
            batch.push(line);
            if (batch.length >= BATCH) flush();
        });
        flush();
        self.postMessage({ done: true });
    } catch (error) {
        self.postMessage({ error: String(error?.message ?? error) });
    }
};
