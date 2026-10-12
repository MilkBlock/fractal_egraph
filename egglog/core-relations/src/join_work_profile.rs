//! Opt-in, single-thread block-level diagnostics. No per-row clocks or tracing.
//! Ordinary builds compile out all hooks. Diagnostics never change the plan.
use std::{cell::RefCell, collections::BTreeSet, sync::OnceLock, time::Instant};

thread_local! { static STAGES: RefCell<Option<Vec<usize>>> = const { RefCell::new(None) }; }

pub(crate) fn record_stage(index: usize) {
    STAGES.with_borrow_mut(|stages| {
        if let Some(counts) = stages {
            counts[index] += 1;
        }
    });
}

pub(crate) struct BlockProfile<'a> {
    rule: &'a str,
    kind: &'static str,
    block: usize,
    stages: usize,
    start: Instant,
    before: Vec<(&'static str, usize)>,
}

fn selected() -> &'static BTreeSet<String> {
    static SELECTED: OnceLock<BTreeSet<String>> = OnceLock::new();
    SELECTED.get_or_init(|| {
        std::env::var("EGGLOG_JOIN_WORK_RULES")
            .unwrap_or_default()
            .split(',')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    })
}

pub(crate) fn verify_threads() {
    if !selected().is_empty() {
        assert_eq!(
            rayon::current_num_threads(),
            1,
            "join-work-profile requires one thread: global counter deltas cannot attribute parallel rules"
        );
    }
}

impl<'a> BlockProfile<'a> {
    pub(crate) fn begin(
        rule: &'a str,
        kind: &'static str,
        block: usize,
        stages: usize,
    ) -> Option<Self> {
        let selected = selected();
        if !selected.contains(rule) && !selected.contains("*") {
            return None;
        }
        assert_eq!(
            rayon::current_num_threads(),
            1,
            "join-work-profile requires one thread: global counter deltas cannot attribute parallel rules"
        );
        if kind != "plan" {
            STAGES.with_borrow_mut(|s| *s = Some(vec![0; stages]));
        }
        Some(Self {
            rule,
            kind,
            block,
            stages,
            start: Instant::now(),
            before: crate::probe_counters::snapshot(),
        })
    }

    pub(crate) fn finish(self, materialized: Option<(usize, usize, usize, usize)>) {
        let nanos = self.start.elapsed().as_nanos();
        let counters = crate::probe_counters::snapshot()
            .into_iter()
            .zip(self.before)
            .map(|((name, n), (old_name, old))| {
                assert_eq!(name, old_name);
                format!("{name}={}", n - old)
            })
            .collect::<Vec<_>>()
            .join("\t");
        // Hex UTF-8 keeps arbitrary rule names safe in this compact TSV stream.
        let rule = self
            .rule
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let materialized = materialized
            .map(|(keys, rows, msg, val)| format!("{keys}\t{rows}\t{msg}\t{val}"))
            .unwrap_or_else(|| "-\t-\t-\t-".to_owned());
        let entries = if self.kind == "plan" {
            Vec::new()
        } else {
            STAGES.with_borrow_mut(Option::take).unwrap_or_default()
        };
        let entries = entries
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(",");
        eprintln!(
            "EGGLOG_JOIN_WORK\t{rule}\t{}\t{}\t{}\t{nanos}\t{materialized}\t{counters}\tstage_entries={entries}",
            self.kind, self.block, self.stages
        );
    }
}
