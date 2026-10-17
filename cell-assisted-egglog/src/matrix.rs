//! Canonical typed slot identities -> exact residual compatibility matrices.
//! NOT a whole-graph adjacency matrix. There are no native table reads here.
use crate::Result;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

pub trait Matcher {
    fn match_rows(&mut self, left: &[Vec<u32>], right: &[Vec<u32>]) -> Result<Vec<(usize, usize)>>;
}
#[derive(Default)]
pub struct Cpu;
impl Matcher for Cpu {
    fn match_rows(&mut self, a: &[Vec<u32>], b: &[Vec<u32>]) -> Result<Vec<(usize, usize)>> {
        validate(a, b)?;
        Ok(a.iter()
            .enumerate()
            .flat_map(|(i, x)| {
                b.iter()
                    .enumerate()
                    .filter_map(move |(j, y)| (x == y).then_some((i, j)))
            })
            .collect())
    }
}
fn validate(a: &[Vec<u32>], b: &[Vec<u32>]) -> Result<usize> {
    let width = a.first().or(b.first()).map_or(0, Vec::len);
    if a.iter().chain(b).any(|r| r.len() != width) {
        return Err("ragged residual matrix".into());
    }
    if width.checked_mul(32).is_none_or(|n| n > (1 << 24)) {
        return Err("residual dot product exceeds the exact FP32 integer range".into());
    }
    Ok(width)
}

/// One persistent Metal/MPS worker. 32 signed bits per native identity; a dot
/// product equals 32*width iff all shared typed slots agree. No hash collisions,
/// FP16 or approximate acceptance. Tiles bound transport memory, never coverage.
/// A failed worker is an error, not a CPU fallback disguised as GPU execution.
pub struct Metal {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    pub device: String,
    pub tiles: usize,
}
impl Metal {
    pub fn new(worker: &Path) -> Result<Self> {
        let mut child = Command::new(worker)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let input = child.stdin.take().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let mut device = String::new();
        output.read_line(&mut device)?;
        if !device.starts_with("READY ") {
            return Err(format!("Metal worker failed: {device}").into());
        }
        Ok(Self {
            child,
            input,
            output,
            device: device.trim().strip_prefix("READY ").unwrap().into(),
            tiles: 0,
        })
    }
}
impl Drop for Metal {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Matcher for Metal {
    fn match_rows(&mut self, a: &[Vec<u32>], b: &[Vec<u32>]) -> Result<Vec<(usize, usize)>> {
        let width = validate(a, b)?;
        if width == 0 {
            return Cpu.match_rows(a, b);
        } // known rank-one block, no GEMM
        let mut pairs = vec![];
        for (ia, aa) in a.chunks(256).enumerate() {
            for (ib, bb) in b.chunks(256).enumerate() {
                let mut frame = Vec::with_capacity(12 + (aa.len() + bb.len()) * width * 4);
                for n in [aa.len() as u32, bb.len() as u32, width as u32] {
                    frame.extend(n.to_le_bytes());
                }
                for v in aa.iter().chain(bb).flatten() {
                    frame.extend(v.to_le_bytes());
                }
                self.input.write_all(&frame)?;
                self.input.flush()?;
                let mut bytes = vec![0; aa.len() * bb.len() * 4];
                self.output.read_exact(&mut bytes)?;
                for (k, word) in bytes.chunks_exact(4).enumerate() {
                    let score = f32::from_le_bytes(word.try_into().unwrap());
                    if !score.is_finite() {
                        return Err("nonfinite Metal result".into());
                    }
                    if score == (width * 32) as f32 {
                        pairs.push((ia * 256 + k / bb.len(), ib * 256 + k % bb.len()));
                    }
                }
                self.tiles += 1;
            }
        }
        pairs.sort_unstable();
        Ok(pairs)
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn bit_dot_is_exact_including_large_ids() {
        let code = |v: u32| -> Vec<i32> {
            (0..32)
                .map(|i| if v >> i & 1 == 0 { -1 } else { 1 })
                .collect()
        };
        for a in [0, 1, 16777216, u32::MAX] {
            for b in [0, 1, 16777217, u32::MAX] {
                let dot: i32 = code(a).iter().zip(code(b)).map(|(x, y)| x * y).sum();
                assert_eq!(dot == 32, a == b);
            }
        }
    }
}
