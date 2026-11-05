//! Calibrate an entire selected marker set. Shared prefixes make the rebuild
//! delta for the bundle unsuitable as a charge on every individual view.
use super::{Result, options, sha};
use serde_json::{Value as J, json};
use std::{fs, io::Write, path::Path, process::Command};

const SCHEMA: &str = "eggfuse-native-marker-cost/v1";
fn phases(binary: &Path, input: &Path) -> Result<[f64; 3]> {
    let out = Command::new(binary)
        .arg(input)
        .env("RAYON_NUM_THREADS", "1")
        .output()?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned().into());
    }
    let stdout = String::from_utf8(out.stdout)?;
    if !stdout
        .lines()
        .last()
        .is_some_and(|l| l.ends_with("[0, 0, 0, 0, 0]"))
    {
        return Err("expected native count_fj timing adapter with counters compiled out".into());
    }
    let line = stdout
        .lines()
        .find_map(|l| l.strip_prefix("PHASE "))
        .ok_or("missing native PHASE")?;
    let times = line
        .split_whitespace()
        .map(str::parse::<f64>)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(times
        .try_into()
        .map_err(|_| "expected search/apply, merge and rebuild times")?)
}
pub(super) fn calibrate(args: Vec<String>) -> Result<()> {
    let opts = options(
        args.into_iter(),
        &["--compiled", "--native", "--output", "--pairs"],
    )?;
    let opt = |k: &str| {
        opts.get(k)
            .map(Path::new)
            .ok_or_else(|| format!("missing {k}"))
    };
    let dir = opt("--compiled")?;
    let binary = opt("--native")?;
    let output = opt("--output")?;
    if output.exists() {
        return Err("native marker profile output already exists".into());
    }
    let pairs = opts
        .get("--pairs")
        .map(|s| s.parse::<usize>())
        .transpose()?
        .unwrap_or(3);
    if !(3..=99).contains(&pairs) {
        return Err("expected 3..99 measured pairs".into());
    }
    let src = [dir.join("source_sk.egg"), dir.join("source_sk_mk.egg")];
    let mut samples = vec![];
    for i in 0..=pairs {
        let mut pair = [[0.; 3]; 2];
        for k in [i % 2, 1 - i % 2] {
            pair[k] = phases(binary, &src[k])?;
        }
        if i != 0 {
            samples.push(pair);
        }
    }
    let profile = json!({"schema":SCHEMA,"source_sha256":sha(&fs::read(&src[0])?),"marked_sha256":sha(&fs::read(&src[1])?),"native_binary_sha256":sha(&fs::read(binary)?),"phase_order_ms":["search_apply","merge","rebuild"],"pairs":samples});
    decision(
        &profile,
        &fs::read_to_string(&src[0])?,
        &fs::read_to_string(&src[1])?,
    )?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&serde_json::to_vec_pretty(&profile)?)?;
    file.persist_noclobber(output)?;
    Ok(())
}
pub(super) fn decision(profile: &J, plain: &str, marked: &str) -> Result<(bool, J)> {
    if profile["schema"] != SCHEMA
        || profile["source_sha256"] != sha(plain.as_bytes())
        || profile["marked_sha256"] != sha(marked.as_bytes())
        || profile["phase_order_ms"] != json!(["search_apply", "merge", "rebuild"])
    {
        return Err(
            "native marker profile differs from proposed source pair or phase schema".into(),
        );
    }
    let samples: Vec<[[f64; 3]; 2]> = serde_json::from_value(profile["pairs"].clone())?;
    if samples.len() < 3
        || samples
            .iter()
            .flatten()
            .flatten()
            .any(|x| !x.is_finite() || *x < 0.)
    {
        return Err("invalid paired native marker timings".into());
    }
    let mut deltas = vec![];
    let mut rebuild = vec![];
    for [before, after] in &samples {
        deltas.push(after.iter().sum::<f64>() - before.iter().sum::<f64>());
        rebuild.push(after[2] - before[2]);
    }
    fn median(mut a: Vec<f64>) -> f64 {
        a.sort_by(f64::total_cmp);
        (a[(a.len() - 1) / 2] + a[a.len() / 2]) / 2.
    }
    let delta = median(deltas);
    let accept = delta < 0.;
    Ok((
        accept,
        json!({"admitted":accept,"pairs":samples.len(),"paired_delta_ms":delta,"paired_rebuild_delta_ms":median(rebuild),"profile":profile}),
    ))
}
