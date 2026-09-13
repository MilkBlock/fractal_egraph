use egg_layout::pattern_store::{Node, RawStore, SharedStore};
use serde_json::json;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    io::{BufReader, Read},
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};
struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
fn add(n: usize) {
    let live = LIVE.fetch_add(n, Ordering::Relaxed) + n;
    PEAK.fetch_max(live, Ordering::Relaxed);
    ALLOCS.fetch_add(1, Ordering::Relaxed);
}
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            add(l.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(l) };
        if !p.is_null() {
            add(l.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        LIVE.fetch_sub(l.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, n) };
        if !q.is_null() {
            LIVE.fetch_sub(l.size(), Ordering::Relaxed);
            add(n);
        }
        q
    }
}
#[global_allocator]
static A: Counting = Counting;
fn u8r(r: &mut impl Read) -> u8 {
    let mut b = [0];
    r.read_exact(&mut b).unwrap();
    b[0]
}
fn u32r(r: &mut impl Read) -> u32 {
    let mut b = [0; 4];
    r.read_exact(&mut b).unwrap();
    u32::from_le_bytes(b)
}
fn node(r: &mut impl Read) -> Node {
    let mut b = [0; 2];
    r.read_exact(&mut b).unwrap();
    let mut n = Node {
        op: u16::from_le_bytes(b),
        arity: u8r(r),
        children: [0; 6],
    };
    assert!(n.arity <= 6);
    for c in &mut n.children[..n.arity as usize] {
        *c = u32r(r);
    }
    n
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let mode = args.get(1).expect("mode").as_str();
    assert!(["raw", "detect", "shared_rehash", "shared_zobrist"].contains(&mode));
    let file = std::fs::File::open(args.get(2).expect("stream")).unwrap();
    let mut input = BufReader::with_capacity(64 * 1024, file);
    let mut magic = [0; 4];
    input.read_exact(&mut magic).unwrap();
    assert_eq!(&magic, b"ZST1");
    let verify = args.get(3).is_some_and(|s| s == "verify");
    let initial = LIVE.load(Ordering::Relaxed);
    PEAK.store(initial, Ordering::Relaxed);
    let t = Instant::now();
    let mut raw = (mode == "raw" || mode == "detect").then(RawStore::default);
    let mut shared = (mode != "raw").then(|| SharedStore::new(mode != "shared_rehash", 64));
    let mut widths = Vec::new();
    let mut roots = Vec::new();
    let mut updates = 0u64;
    let mut changed = 0u64;
    let mut checked = 0usize;
    let mut update_ns = 0u128;
    let roots_expected = loop {
        match u8r(&mut input) {
            b'C' => {
                let class = u32r(&mut input) as usize;
                assert_eq!(class, widths.len());
                widths.push(u8r(&mut input));
                let root = u32r(&mut input);
                roots.push(root);
                if let Some(s) = &mut raw {
                    s.add_class();
                }
                if let Some(s) = &mut shared {
                    s.add_class(Vec::new());
                }
            }
            b'R' => {
                let class = u32r(&mut input) as usize;
                roots[class] = u32r(&mut input);
            }
            b'U' => {
                let class = u32r(&mut input) as usize;
                let insert = u8r(&mut input) != 0;
                let n = node(&mut input);
                let start = Instant::now();
                let a = raw
                    .as_mut()
                    .map(|s| s.update(class, widths[class] as usize, n, insert));
                let b = shared.as_mut().map(|s| s.update(class, n, insert));
                update_ns += start.elapsed().as_nanos();
                if let (Some(a), Some(b)) = (a, b) {
                    assert_eq!(a, b);
                }
                changed += u64::from(a.or(b).unwrap());
                updates += 1;
            }
            b'V' => {
                let class = u32r(&mut input) as usize;
                let root = u32r(&mut input);
                assert_eq!(root, roots[class]);
                let len = u32r(&mut input) as usize;
                if verify {
                    let mut expected: Vec<_> = (0..len).map(|_| node(&mut input)).collect();
                    expected.sort();
                    if let Some(s) = &raw {
                        let mut actual = s.decode(class, widths[class] as usize);
                        actual.sort();
                        assert_eq!(actual, expected);
                    }
                    if let Some(s) = &shared {
                        let mut actual = s.decode(class);
                        actual.sort();
                        assert_eq!(actual, expected);
                    }
                    checked += 1;
                } else {
                    for _ in 0..len {
                        let _ = node(&mut input);
                    }
                }
            }
            b'E' => {
                break u32r(&mut input) as u64;
            }
            tag => panic!("unknown tag {tag}"),
        }
    };
    let build_ns = t.elapsed().as_nanos();
    let precompact_bytes = LIVE.load(Ordering::Relaxed) - initial;
    let tc = Instant::now();
    if let Some(s) = &mut raw {
        s.compact();
    }
    if let Some(s) = &mut shared {
        s.compact();
    }
    roots.shrink_to_fit();
    widths.shrink_to_fit();
    let compact_ns = tc.elapsed().as_nanos();
    let retained = LIVE.load(Ordering::Relaxed) - initial;
    let peak = PEAK.load(Ordering::Relaxed) - initial;
    let ts = Instant::now();
    let mut checksum = 0u64;
    for _ in 0..5 {
        let a = raw.as_ref().map(|s| std::hint::black_box(s.scan(&widths)));
        let b = shared.as_ref().map(|s| std::hint::black_box(s.scan()));
        if let (Some(a), Some(b)) = (a, b) {
            assert_eq!(a, b);
        }
        checksum = checksum.wrapping_add(a.or(b).unwrap());
    }
    let scan_ns = ts.elapsed().as_nanos() / 5;
    assert_eq!(roots.len(), roots_expected as usize);
    let stats=shared.as_ref().map(|s|json!({"hits":s.stats.hits,"probes":s.stats.probes,"exact_comparisons":s.stats.exact_comparisons,"hash_terms":s.stats.hash_terms,"live_templates":s.stats.live_templates,"peak_templates":s.stats.peak_templates}));
    println!(
        "{}",
        json!({"mode":mode,"verified":verify,"classes":roots.len(),"updates":updates,"changed":changed,"checked_snapshots":checked,"retained_bytes":retained,"precompact_bytes":precompact_bytes,"peak_requested_bytes":peak,"update_ns":update_ns,"build_ns":build_ns,"compact_ns":compact_ns,"scan_ns":scan_ns,"checksum":checksum,"stats":stats,"allocation_calls":ALLOCS.load(Ordering::Relaxed)})
    );
}
