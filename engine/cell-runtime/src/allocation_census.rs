//! Diagnostic allocation-origin accounting. Requested live bytes count each
//! allocation once, including shared Rc payloads. Category peaks need not
//! coincide. Allocator metadata, fragmentation and C allocations are excluded.
use std::{
    alloc::{GlobalAlloc, Layout},
    cell::Cell,
    ptr,
    sync::atomic::{AtomicU64, Ordering::Relaxed},
};
#[repr(usize)]
#[derive(Clone, Copy)]
pub enum Tag {
    Other,
    Artifact,
    Graph,
    Rows,
    Search,
    Rhs,
    Columns,
    Entries,
    Sieve,
    Spine,
    History,
    Context,
    Primitive,
}
const N: usize = 13;
const NAMES: [&str; N] = [
    "other",
    "artifact",
    "graph",
    "row_payload",
    "search",
    "rhs",
    "columns",
    "entries",
    "sieve",
    "spine",
    "history",
    "local_context",
    "primitives",
];
thread_local! {static CURRENT:Cell<usize>=const{Cell::new(0)};}
pub struct Scope(usize);
pub fn scope(tag: Tag) -> Scope {
    Scope(CURRENT.with(|s| s.replace(tag as usize)))
}
impl Drop for Scope {
    fn drop(&mut self) {
        let _ = CURRENT.try_with(|s| s.set(self.0));
    }
}
struct Counter {
    live: AtomicU64,
    peak: AtomicU64,
    blocks: AtomicU64,
    allocations: AtomicU64,
    overhead: AtomicU64,
}
impl Counter {
    const fn new() -> Self {
        Self {
            live: AtomicU64::new(0),
            peak: AtomicU64::new(0),
            blocks: AtomicU64::new(0),
            allocations: AtomicU64::new(0),
            overhead: AtomicU64::new(0),
        }
    }
}
static COUNTERS: [Counter; N] = [const { Counter::new() }; N];
static TOTAL: AtomicU64 = AtomicU64::new(0);
static PEAK: AtomicU64 = AtomicU64::new(0);
fn add(tag: usize, size: usize, overhead: usize) {
    let c = &COUNTERS[tag];
    let live = c.live.fetch_add(size as u64, Relaxed) + size as u64;
    c.peak.fetch_max(live, Relaxed);
    c.blocks.fetch_add(1, Relaxed);
    c.allocations.fetch_add(1, Relaxed);
    c.overhead.fetch_add(overhead as u64, Relaxed);
    PEAK.fetch_max(TOTAL.fetch_add(size as u64, Relaxed) + size as u64, Relaxed);
}
fn sub(tag: usize, size: usize, overhead: usize) {
    let c = &COUNTERS[tag];
    c.live.fetch_sub(size as u64, Relaxed);
    c.blocks.fetch_sub(1, Relaxed);
    c.overhead.fetch_sub(overhead as u64, Relaxed);
    TOTAL.fetch_sub(size as u64, Relaxed);
}
#[repr(C)]
struct Header {
    tag: usize,
}
fn layout(l: Layout) -> Option<(Layout, usize)> {
    let (full, offset) = Layout::new::<Header>().extend(l).ok()?;
    Some((full.pad_to_align(), offset))
}
pub struct Allocator;
// The prefix is inside the same allocation, immediately before the aligned
// user region. GlobalAlloc supplies the original layout for deallocation.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let Some((full, offset)) = layout(l) else {
            return ptr::null_mut();
        };
        let base = unsafe { mimalloc::MiMalloc.alloc(full) };
        if base.is_null() {
            return base;
        }
        let out = unsafe { base.add(offset) };
        let tag = CURRENT.try_with(Cell::get).unwrap_or(0);
        unsafe {
            out.sub(size_of::<Header>())
                .cast::<Header>()
                .write(Header { tag });
        }
        add(tag, l.size(), full.size() - l.size());
        out
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let out = unsafe { self.alloc(l) };
        if !out.is_null() {
            unsafe { out.write_bytes(0, l.size()) }
        }
        out
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        let (full, offset) = layout(l).expect("previously allocated layout");
        let tag = unsafe { p.sub(size_of::<Header>()).cast::<Header>().read().tag };
        sub(tag, l.size(), full.size() - l.size());
        unsafe {
            mimalloc::MiMalloc.dealloc(p.sub(offset), full);
        }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let Ok(next) = Layout::from_size_align(n, l.align()) else {
            return ptr::null_mut();
        };
        let Some((new_layout, new_offset)) = layout(next) else {
            return ptr::null_mut();
        };
        let (old_layout, offset) = layout(l).unwrap();
        debug_assert_eq!(new_offset, offset);
        let tag = unsafe { p.sub(size_of::<Header>()).cast::<Header>().read().tag };
        let base =
            unsafe { mimalloc::MiMalloc.realloc(p.sub(offset), old_layout, new_layout.size()) };
        if base.is_null() {
            return base;
        }
        sub(tag, l.size(), old_layout.size() - l.size());
        add(tag, n, new_layout.size() - n);
        unsafe { base.add(offset) }
    }
}
pub fn snapshot() -> serde_json::Value {
    let counts = std::array::from_fn::<_, N, _>(|i| {
        let c = &COUNTERS[i];
        [
            c.live.load(Relaxed),
            c.peak.load(Relaxed),
            c.blocks.load(Relaxed),
            c.allocations.load(Relaxed),
            c.overhead.load(Relaxed),
        ]
    });
    let total = TOTAL.load(Relaxed);
    let peak = PEAK.load(Relaxed);
    serde_json::json!({"requested_live_bytes":total,"requested_peak_bytes":peak,"categories":NAMES.iter().zip(counts).map(|(name,c)|serde_json::json!({"category":name,"live_bytes":c[0],"peak_bytes":c[1],"live_allocations":c[2],"allocation_calls_including_realloc":c[3],"instrumentation_overhead_live_bytes":c[4]})).collect::<Vec<_>>()})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aligned_realloc_retains_bytes_and_the_original_category() {
        let saved = COUNTERS[Tag::History as usize].live.load(Relaxed);
        let a = Allocator;
        let scope = scope(Tag::History);
        for align in [1, 8, 64, 256] {
            let l = Layout::from_size_align(37, align).unwrap();
            let p = unsafe { a.alloc_zeroed(l) };
            assert!(!p.is_null());
            assert_eq!(p as usize % align, 0);
            assert!(
                unsafe { std::slice::from_raw_parts(p, 37) }
                    .iter()
                    .all(|x| *x == 0)
            );
            unsafe {
                p.write_bytes(19, 37);
            }
            let other = super::scope(Tag::Other);
            let q = unsafe { a.realloc(p, l, 513) };
            assert!(!q.is_null());
            assert_eq!(q as usize % align, 0);
            assert!(
                unsafe { std::slice::from_raw_parts(q, 37) }
                    .iter()
                    .all(|x| *x == 19)
            );
            unsafe { a.dealloc(q, Layout::from_size_align(513, align).unwrap()) }
            drop(other);
        }
        drop(scope);
        assert_eq!(COUNTERS[Tag::History as usize].live.load(Relaxed), saved);
    }
}
