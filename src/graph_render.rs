//! Presentation work is deferred until analysis completes, then rendered in a
//! dedicated bounded pool. Only filenames are queued, never native EGraphs.
use crate::pipeline::Result;
use rayon::prelude::*;
use std::{
    cell::RefCell,
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::Command,
};
#[derive(Default)]
struct Jobs {
    dots: BTreeSet<PathBuf>,
    copies: Vec<(PathBuf, PathBuf)>,
}
thread_local! { static JOBS: RefCell<Option<Jobs>> = const { RefCell::new(None) }; }
pub(crate) struct Batch {
    workers: usize,
}
impl Batch {
    pub fn start() -> Result<Self> {
        let workers = std::env::var("EGG_LAYOUT_SVG_THREADS")
            .ok()
            .map(|s| s.parse::<usize>())
            .transpose()?
            .unwrap_or(2);
        if workers == 0 {
            return Err("EGG_LAYOUT_SVG_THREADS must be positive".into());
        }
        JOBS.with(|j| {
            let mut j = j.borrow_mut();
            assert!(j.is_none(), "nested render batch");
            *j = Some(Jobs::default());
        });
        Ok(Self { workers })
    }
    pub fn finish(self) -> Result<()> {
        let jobs = JOBS.with(|j| j.borrow_mut().take()).unwrap();
        if jobs.dots.is_empty() && jobs.copies.is_empty() {
            return Ok(());
        }
        let started = std::time::Instant::now();
        eprintln!(
            "[svg] rendering {} graphs with {} workers",
            jobs.dots.len(),
            self.workers
        );
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(self.workers)
            .build()?;
        // Complete all tasks before publishing success; return failures to the caller.
        let results: Vec<_> = pool.install(|| {
            jobs.dots
                .par_iter()
                .map(|dot| render(dot).map_err(|e| e.to_string()))
                .collect()
        });
        for result in results {
            result.map_err(|e| format!("SVG rendering failed: {e}"))?;
        }
        for (src, dst) in jobs.copies {
            std::fs::copy(src, dst)?;
        }
        eprintln!("[svg] completed in {:.3}s", started.elapsed().as_secs_f64());
        Ok(())
    }
}
impl Drop for Batch {
    fn drop(&mut self) {
        JOBS.with(|j| {
            j.borrow_mut().take();
        });
    }
}
fn enabled() -> bool {
    std::env::var("EGG_LAYOUT_EAGER_SVG").as_deref() != Ok("0")
}
pub(crate) fn enqueue(dot: &Path) -> Result<()> {
    if !enabled() {
        return Ok(());
    }
    let queued = JOBS.with(|j| {
        if let Some(j) = j.borrow_mut().as_mut() {
            j.dots.insert(dot.to_owned());
            true
        } else {
            false
        }
    });
    if !queued {
        render(dot)?;
    }
    Ok(())
}
pub(crate) fn copy(src: &Path, dst: &Path) -> Result<()> {
    if !enabled() {
        return Ok(());
    }
    let queued = JOBS.with(|j| {
        if let Some(j) = j.borrow_mut().as_mut() {
            j.copies.push((src.to_owned(), dst.to_owned()));
            true
        } else {
            false
        }
    });
    if !queued && src.exists() {
        std::fs::copy(src, dst)?;
    }
    Ok(())
}
fn render(dot: &Path) -> Result<()> {
    let svg = dot.with_extension("svg");
    let tmp = svg.with_extension("svg.partial");
    let result = Command::new("dot")
        .arg("-Tsvg")
        .arg(dot)
        .arg("-o")
        .arg(&tmp)
        .output()?;
    if !result.status.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "{}: {}",
            dot.display(),
            String::from_utf8_lossy(&result.stderr)
        )
        .into());
    }
    std::fs::rename(tmp, svg)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn batch_defers_deduplicates_and_copies_completed_svg() {
        let dir = std::env::temp_dir().join(format!("svg-batch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let batch = Batch::start().unwrap();
        for name in ["a", "b"] {
            let dot = dir.join(format!("{name}.dot"));
            std::fs::write(&dot, "digraph { a -> b }").unwrap();
            enqueue(&dot).unwrap();
            enqueue(&dot).unwrap();
            assert!(!dot.with_extension("svg").exists());
        }
        JOBS.with(|j| assert_eq!(j.borrow().as_ref().unwrap().dots.len(), 2));
        copy(&dir.join("a.svg"), &dir.join("alias.svg")).unwrap();
        batch.finish().unwrap();
        assert_eq!(
            std::fs::read(dir.join("a.svg")).unwrap(),
            std::fs::read(dir.join("alias.svg")).unwrap()
        );
        assert!(
            std::fs::read_to_string(dir.join("b.svg"))
                .unwrap()
                .contains("<svg")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn failure_is_reported_without_publishing_partial_svg() {
        let dir = std::env::temp_dir().join(format!("svg-error-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let batch = Batch::start().unwrap();
        let dot = dir.join("bad.dot");
        std::fs::write(&dot, "not a graph").unwrap();
        enqueue(&dot).unwrap();
        assert!(batch.finish().is_err());
        assert!(!dot.with_extension("svg").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
