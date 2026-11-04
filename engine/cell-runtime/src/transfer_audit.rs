//! Untimed inference counters. The release benchmark compiles without this feature.
use crate::value::Value;
use std::cell::Cell;
#[derive(Clone, Copy, Default, serde::Serialize)]
pub struct Counts {
    pub decoded_rows: u64,
    pub row_fields: u64,
    pub fresh_bind_copies: u64,
    pub binding_seal_copies: u64,
    pub parent_projected_fields: u64,
    pub owned_frame_fields: u64,
    pub value_clones: u64,
}
thread_local! {
    static DIGESTS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static COUNTS: Cell<Counts> = Cell::default();
}
pub fn active() -> bool {
    ACTIVE.get()
}
pub fn set_active(on: bool) {
    ACTIVE.set(on);
}
pub fn counts() -> Counts {
    COUNTS.get()
}
pub(crate) fn count(f: impl FnOnce(&mut Counts)) {
    if active() {
        let mut c = counts();
        f(&mut c);
        COUNTS.set(c);
    }
}
pub(crate) fn row(fields: usize) {
    count(|c| {
        c.decoded_rows += 1;
        c.row_fields += fields as u64;
    });
}
impl Clone for Value {
    fn clone(&self) -> Self {
        count(|c| c.value_clones += 1);
        match self {
            Self::Eq(x) => Self::Eq(*x),
            Self::Int(x) => Self::Int(*x),
            Self::Float(x) => Self::Float(*x),
            Self::Text(x) => Self::Text(x.clone()),
            Self::Set(x) => Self::Set(x.clone()),
            Self::Key(x) => Self::Key(x.clone()),
        }
    }
}
pub(crate) fn record_run(rows: impl Iterator<Item = impl serde::Serialize>) -> Result<(), String> {
    if active() {
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
