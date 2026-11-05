//! Ordered RHS inputs for each Run, used by iterative Bake and the audit build.
use std::cell::Cell;
thread_local! {
    static DIGESTS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
}
pub fn set_active(on: bool) {
    ACTIVE.set(on);
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
