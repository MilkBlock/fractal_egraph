//! Experimental lossless local-node store, not an egglog table replacement.
//! Stable, explicitly stored port bindings make incremental Zobrist updates possible.
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Node {
    pub op: u16,
    pub arity: u8,
    pub children: [u32; 6],
}
impl Node {
    fn row(&self) -> [u32; 7] {
        let mut row = [0; 7];
        row[0] = u32::from(self.op) | (u32::from(self.arity) << 16);
        row[1..].copy_from_slice(&self.children);
        row
    }
}

#[derive(Default)]
pub struct RawStore {
    rows: Vec<Vec<u32>>,
}
impl RawStore {
    pub fn add_class(&mut self) {
        self.rows.push(Vec::new());
    }
    pub fn update(&mut self, class: usize, width: usize, node: Node, insert: bool) -> bool {
        let key = node.row();
        let key = &key[..width];
        let rows = &mut self.rows[class];
        let (mut lo, mut hi) = (0, rows.len() / width);
        while lo < hi {
            let mid = (lo + hi) / 2;
            if &rows[mid * width..(mid + 1) * width] < key {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let pos = lo * width;
        let exists = pos < rows.len() && &rows[pos..pos + width] == key;
        if insert && !exists {
            rows.splice(pos..pos, key.iter().copied());
            true
        } else if !insert && exists {
            rows.drain(pos..pos + width);
            true
        } else {
            false
        }
    }
    pub fn decode(&self, class: usize, width: usize) -> Vec<Node> {
        self.rows[class]
            .chunks_exact(width)
            .map(|row| {
                let mut node = Node {
                    op: row[0] as u16,
                    arity: (row[0] >> 16) as u8,
                    children: [0; 6],
                };
                node.children[..width - 1].copy_from_slice(&row[1..]);
                node
            })
            .collect()
    }
    pub fn scan(&self, widths: &[u8]) -> u64 {
        let mut sum = 0u64;
        for (rows, width) in self.rows.iter().zip(widths) {
            for row in rows.chunks_exact(*width as usize) {
                sum = sum.wrapping_add(row[0] as u16 as u64);
                for c in &row[1..1 + (row[0] >> 16) as usize] {
                    sum = sum.wrapping_add(*c as u64);
                }
            }
        }
        sum
    }
    pub fn compact(&mut self) {
        for row in &mut self.rows {
            row.shrink_to_fit();
        }
        self.rows.shrink_to_fit();
    }
}

#[derive(Default, Debug)]
pub struct Stats {
    pub probes: u64,
    pub exact_comparisons: u64,
    pub hits: u64,
    pub hash_terms: u64,
    pub live_templates: usize,
    pub peak_templates: usize,
}
struct Template {
    nodes: Box<[u64]>,
    hash: u64,
    refs: usize,
}
struct Instance {
    bindings: Box<[u32]>,
    template: usize,
}
pub struct SharedStore {
    classes: Vec<Instance>,
    templates: Vec<Option<Template>>,
    free: Vec<usize>,
    buckets: HashMap<(u64, usize), Vec<usize>>,
    incremental: bool,
    hash_bits: u32,
    pub stats: Stats,
}
fn zobrist(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e3779b97f4a7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}
impl SharedStore {
    pub fn new(incremental: bool, hash_bits: u32) -> Self {
        assert!(hash_bits <= 64);
        let mut buckets = HashMap::new();
        buckets.insert((0, 0), vec![0]);
        Self {
            classes: Vec::new(),
            templates: vec![Some(Template {
                nodes: Box::new([]),
                hash: 0,
                refs: 0,
            })],
            free: Vec::new(),
            buckets,
            incremental,
            hash_bits,
            stats: Stats {
                live_templates: 1,
                peak_templates: 1,
                ..Default::default()
            },
        }
    }
    pub fn add_class(&mut self, bindings: Vec<u32>) {
        assert!(bindings.len() <= 128);
        assert!(
            bindings
                .iter()
                .enumerate()
                .all(|(i, x)| !bindings[..i].contains(x))
        );
        self.classes.push(Instance {
            bindings: bindings.into_boxed_slice(),
            template: 0,
        });
        self.templates[0].as_mut().unwrap().refs += 1;
    }
    fn token(&self, class: usize, node: Node) -> u64 {
        assert!(node.arity <= 6);
        let mut token = u64::from(node.op) | (u64::from(node.arity) << 16);
        for i in 0..node.arity as usize {
            let slot = self.classes[class]
                .bindings
                .iter()
                .position(|v| *v == node.children[i])
                .expect("unbound child");
            token |= (slot as u64) << (19 + 7 * i);
        }
        token
    }
    fn fingerprint(&self, token: u64) -> u64 {
        let h = zobrist(token);
        if self.hash_bits == 64 {
            h
        } else {
            h & ((1u64 << self.hash_bits) - 1)
        }
    }
    pub fn update(&mut self, class: usize, node: Node, insert: bool) -> bool {
        if insert {
            for child in &node.children[..node.arity as usize] {
                if !self.classes[class].bindings.contains(child) {
                    let mut bindings = self.classes[class].bindings.to_vec();
                    assert!(bindings.len() < 128);
                    bindings.push(*child);
                    self.classes[class].bindings = bindings.into_boxed_slice();
                }
            }
        } else if node.children[..node.arity as usize]
            .iter()
            .any(|c| !self.classes[class].bindings.contains(c))
        {
            return false;
        }
        let token = self.token(class, node);
        let old_id = self.classes[class].template;
        let old = self.templates[old_id].as_ref().unwrap();
        let position = old.nodes.binary_search(&token);
        let pos = match (insert, position) {
            (true, Err(pos)) | (false, Ok(pos)) => pos,
            _ => return false,
        };
        let old_hash = old.hash;
        let mut nodes = Vec::with_capacity(if insert {
            old.nodes.len() + 1
        } else {
            old.nodes.len()
        });
        nodes.extend_from_slice(&old.nodes);
        if insert {
            nodes.insert(pos, token);
        } else {
            nodes.remove(pos);
        }
        let hash = if self.incremental {
            self.stats.hash_terms += 1;
            old_hash ^ self.fingerprint(token)
        } else {
            self.stats.hash_terms += nodes.len() as u64;
            nodes.iter().fold(0, |h, t| h ^ self.fingerprint(*t))
        };
        let key = (hash, nodes.len());
        self.stats.probes += 1;
        let mut found = None;
        if let Some(candidates) = self.buckets.get(&key) {
            for id in candidates {
                self.stats.exact_comparisons += 1;
                if self.templates[*id].as_ref().unwrap().nodes.as_ref() == nodes.as_slice() {
                    found = Some(*id);
                    break;
                }
            }
        }
        let id = if let Some(id) = found {
            self.stats.hits += 1;
            self.templates[id].as_mut().unwrap().refs += 1;
            id
        } else {
            let t = Some(Template {
                nodes: nodes.into_boxed_slice(),
                hash,
                refs: 1,
            });
            let id = if let Some(id) = self.free.pop() {
                self.templates[id] = t;
                id
            } else {
                let id = self.templates.len();
                self.templates.push(t);
                id
            };
            self.buckets.entry(key).or_default().push(id);
            self.stats.live_templates += 1;
            self.stats.peak_templates = self.stats.peak_templates.max(self.stats.live_templates);
            id
        };
        self.classes[class].template = id;
        let old = self.templates[old_id].as_mut().unwrap();
        old.refs -= 1;
        if old.refs == 0 && old_id != 0 {
            // No full history of obsolete templates is retained.
            let old_key = (old.hash, old.nodes.len());
            let bucket = self.buckets.get_mut(&old_key).unwrap();
            bucket.retain(|id| *id != old_id);
            if bucket.is_empty() {
                self.buckets.remove(&old_key);
            }
            self.templates[old_id] = None;
            self.free.push(old_id);
            self.stats.live_templates -= 1;
        }
        true
    }
    pub fn decode(&self, class: usize) -> Vec<Node> {
        let inst = &self.classes[class];
        self.templates[inst.template]
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .map(|t| {
                let mut node = Node {
                    op: *t as u16,
                    arity: ((t >> 16) & 7) as u8,
                    children: [0; 6],
                };
                for i in 0..node.arity as usize {
                    node.children[i] = inst.bindings[((t >> (19 + 7 * i)) & 127) as usize];
                }
                node
            })
            .collect()
    }
    pub fn scan(&self) -> u64 {
        let mut sum = 0u64;
        for inst in &self.classes {
            for t in &self.templates[inst.template].as_ref().unwrap().nodes {
                sum = sum.wrapping_add(*t as u16 as u64);
                for i in 0..((t >> 16) & 7) as usize {
                    sum = sum
                        .wrapping_add(inst.bindings[((t >> (19 + 7 * i)) & 127) as usize] as u64);
                }
            }
        }
        sum
    }
    pub fn compact(&mut self) {
        self.classes.shrink_to_fit();
        self.templates.shrink_to_fit();
        self.free.shrink_to_fit();
        self.buckets.shrink_to_fit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn n(a: u32, b: u32) -> Node {
        Node {
            op: 1,
            arity: 2,
            children: [a, b, 0, 0, 0, 0],
        }
    }
    #[test]
    fn collision_and_mutation_differential() {
        // Force EVERY equal-size set into the same hash bucket.
        let mut compressed = SharedStore::new(true, 0);
        let mut raw = RawStore::default();
        for _ in 0..8 {
            compressed.add_class(vec![11, 22, 33]);
            raw.add_class();
        }
        let mut seed = 1u64;
        for _ in 0..4000 {
            seed = zobrist(seed);
            let class = (seed % 8) as usize;
            let node = n(
                [11, 22, 33][((seed >> 8) % 3) as usize],
                [11, 22, 33][((seed >> 12) % 3) as usize],
            );
            let insert = (seed >> 20) & 1 == 0;
            assert_eq!(
                raw.update(class, 3, node, insert),
                compressed.update(class, node, insert)
            );
            let mut a = raw.decode(class, 3);
            let mut b = compressed.decode(class);
            a.sort();
            b.sort();
            assert_eq!(a, b);
        }
        assert_eq!(raw.scan(&[3; 8]), compressed.scan());
    }
    #[test]
    fn repeated_insert_and_shared_copy_on_write() {
        let mut s = SharedStore::new(true, 64);
        s.add_class(vec![10, 20]);
        s.add_class(vec![30, 40]);
        assert!(s.update(0, n(10, 20), true));
        assert!(!s.update(0, n(10, 20), true));
        assert!(s.update(1, n(30, 40), true));
        assert_eq!(s.classes[0].template, s.classes[1].template);
        s.update(0, n(20, 10), true);
        assert_eq!(s.decode(1), vec![n(30, 40)]);
        s.update(0, n(10, 20), false);
        s.update(0, n(20, 10), false);
        assert_eq!(s.classes[0].template, 0);
    }
    #[test]
    fn online_ports_and_canonicalization_style_dedup() {
        let mut s = SharedStore::new(true, 64);
        s.add_class(Vec::new());
        assert!(s.update(0, n(1, 2), true));
        assert!(s.update(0, n(1, 3), true));
        // A child merge can turn a distinct row into an already existing row.
        assert!(s.update(0, n(1, 3), false));
        assert!(!s.update(0, n(1, 2), true));
        assert_eq!(s.decode(0), vec![n(1, 2)]);
        assert_eq!(s.classes[0].bindings.as_ref(), &[1, 2, 3]);
    }

    #[test]
    fn rehash_and_incremental_fingerprints_agree() {
        let mut a = SharedStore::new(true, 64);
        let mut b = SharedStore::new(false, 64);
        a.add_class(vec![1, 2]);
        b.add_class(vec![1, 2]);
        for (node, insert) in [
            (n(1, 2), true),
            (n(2, 1), true),
            (n(1, 2), false),
            (n(2, 1), false),
        ] {
            a.update(0, node, insert);
            b.update(0, node, insert);
            assert_eq!(
                a.templates[a.classes[0].template].as_ref().unwrap().hash,
                b.templates[b.classes[0].template].as_ref().unwrap().hash
            );
        }
    }
}
