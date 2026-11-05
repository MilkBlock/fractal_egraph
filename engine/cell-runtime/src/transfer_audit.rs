//! Ordered RHS inputs for each Run, used by iterative Bake and the audit build.
use std::cell::Cell;
thread_local! {
    static DIGESTS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
}
pub fn set_active(on: bool) {
    ACTIVE.set(on);
    if !on {
        WRITE_ACTIVE.set(false);
        SOURCE.set(None);
    }
}
pub(crate) fn record_run(rows: impl Iterator<Item = impl serde::Serialize>) -> Result<(), String> {
    if ACTIVE.get() {
        use sha2::Digest;
        let mut digest = sha2::Sha256::new();
        for row in rows {
            serde_json::to_writer(&mut digest, &row).map_err(|e| e.to_string())?;
        }
        DIGESTS.with_borrow_mut(|runs| runs.push(format!("{:x}", digest.finalize())));
    }
    Ok(())
}
pub fn take_digests() -> Vec<String> {
    DIGESTS.take()
}

// Keep event order including multiple improvements to the same key in one Run.
thread_local! {
    static WRITE_ACTIVE:Cell<bool>=const{Cell::new(false)};
    static ORDINAL:Cell<u64>=const{Cell::new(0)};
    static SOURCE:Cell<Option<(u64,usize)>>=const{Cell::new(None)};
    static WRITES: std::cell::RefCell<Option<(u64,sha2::Sha256)>>=const{std::cell::RefCell::new(None)};
    static WRITE_RUNS: std::cell::RefCell<Vec<(u64,String)>>=const{std::cell::RefCell::new(Vec::new())};
}
pub fn enable_write_audit() {
    WRITE_ACTIVE.set(true);
}
pub(crate) fn rhs(rid: usize) {
    if WRITE_ACTIVE.get() {
        let n = ORDINAL.get();
        SOURCE.set(Some((n, rid)));
        ORDINAL.set(n + 1);
    }
}
pub(crate) fn repaired() {
    SOURCE.set(None);
}
pub(crate) fn begin_writes() {
    if WRITE_ACTIVE.get() {
        ORDINAL.set(0);
        SOURCE.set(None);
        use sha2::Digest;
        WRITES.replace(Some((0, sha2::Sha256::new())));
    }
}
pub(crate) fn effective(
    epoch: u64,
    table: usize,
    index: usize,
    key: &[crate::value::Value],
    old: Option<&crate::value::Value>,
    new: &crate::value::Value,
) -> Result<(), String> {
    WRITES.with_borrow_mut(|state| {
        if let Some((count, hash)) = state {
            serde_json::to_writer(hash, &(epoch, SOURCE.get(), table, index, key, old, new))
                .map_err(|e| e.to_string())?;
            *count += 1;
        }
        Ok(())
    })
}
pub(crate) fn end_writes() {
    use sha2::Digest;
    if let Some((n, h)) = WRITES.take() {
        WRITE_RUNS.with_borrow_mut(|rs| rs.push((n, format!("{:x}", h.finalize()))));
    }
}
pub fn take_write_digests() -> Vec<(u64, String)> {
    WRITE_RUNS.take()
}
