//! Persistent matrix worker. Installation extends immutable coordinates; device
//! buffers, cached predicates and completed match decisions survive on the GPU.
use crate::{Result, program::Program};
use serde::Serialize;
use serde_json::{Value as Json, json};
use std::{
    io::{BufReader, Read, Write},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::Instant,
};

#[derive(Default, Serialize)]
pub struct InstallTimes {
    pub check_seconds: f64,
    pub planner_seconds: f64,
    pub lower_seconds: f64,
    pub worker_install_seconds: f64,
    /// The following are subsets of worker_install_seconds, not additive phases.
    pub worker_decode_seconds: f64,
    pub worker_buffer_seconds: f64,
    pub worker_shader_compile_seconds: f64,
    pub worker_inherit_seconds: f64,
}

pub struct Service {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    program: Option<Program>,
    pub planner: crate::layout::Planner,
    pub layout_report: Json,
    pub install_times: InstallTimes,
}
impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn write(w: &mut impl Write, v: &Json) -> Result<()> {
    let data = serde_json::to_vec(v)?;
    w.write_all(&u32::try_from(data.len())?.to_le_bytes())?;
    w.write_all(&data)?;
    w.flush()?;
    Ok(())
}
fn read(r: &mut impl Read) -> Result<Json> {
    let mut n = [0; 4];
    r.read_exact(&mut n)?;
    let mut v = vec![0; u32::from_le_bytes(n) as usize];
    r.read_exact(&mut v)?;
    let v: Json = serde_json::from_slice(&v)?;
    if let Some(e) = v.get("error") {
        return Err(format!("matrix worker: {e}").into());
    }
    Ok(v)
}
fn write_words(w: &mut impl Write, words: &[u32]) -> Result<()> {
    let bytes = words.len().checked_mul(4).ok_or("binary frame overflow")?;
    w.write_all(&u32::try_from(bytes)?.to_le_bytes())?;
    let mut buffer = [0u8; 16384];
    for chunk in words.chunks(buffer.len() / 4) {
        for (i, v) in chunk.iter().enumerate() {
            buffer[4 * i..4 * i + 4].copy_from_slice(&v.to_le_bytes());
        }
        w.write_all(&buffer[..chunk.len() * 4])?;
    }
    w.flush()?;
    Ok(())
}
fn read_words(r: &mut impl Read) -> Result<Vec<u32>> {
    let mut n = [0u8; 4];
    r.read_exact(&mut n)?;
    let n = u32::from_le_bytes(n) as usize;
    if n % 4 != 0 {
        return Err("unaligned binary response".into());
    }
    let mut data = vec![0; n];
    r.read_exact(&mut data)?;
    Ok(data
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect())
}
fn extension(old: &Program, new: &Program) -> Result<()> {
    if old.bits != new.bits || new.slots < old.slots || !new.updates.is_empty() {
        return Err("live install cannot change coordinate semantics".into());
    }
    if !new.nodes.starts_with(&old.nodes)
        || !new.cells.starts_with(&old.cells)
        || !new.templates.starts_with(&old.templates)
        || !new.conditions.starts_with(&old.conditions)
        || new.channels.len() < old.channels.len()
        || new.stages.len() < old.stages.len()
    {
        return Err("live program changed immutable prefixes".into());
    }
    for (a, b) in old.channels.iter().zip(&new.channels) {
        if a.label != b.label || a.sources != b.sources || a.alternatives != b.alternatives {
            return Err("live install changed a completed match or its binding".into());
        }
        match (&a.target, &b.target) {
            (None, None) => (),
            (Some(a), Some(b))
                if a.certified == b.certified
                    && a.cell == b.cell
                    && a.bindings == b.bindings
                    && b.specializations.starts_with(&a.specializations) =>
            {
                ()
            }
            _ => return Err("cannot replace a specialization certificate".into()),
        }
    }
    match (&old.numeric, &new.numeric) {
        (Some(_), None) => return Err("cannot remove numeric semantics".into()),
        (a, Some(n)) => {
            if let Some(a) = a {
                if !n.inputs.starts_with(&a.inputs)
                    || !n.state_bindings.starts_with(&a.state_bindings)
                    || !n.exprs.starts_with(&a.exprs)
                    || !n.guards.starts_with(&a.guards)
                    || !n.groups.starts_with(&a.groups)
                    || !n.writes.starts_with(&a.writes)
                {
                    return Err("live install changed numeric semantics".into());
                }
            }
            if n.guards[a.as_ref().map_or(0, |a| a.guards.len())..]
                .iter()
                .any(|g| g.channel < old.channels.len())
                || n.writes[a.as_ref().map_or(0, |a| a.writes.len())..]
                    .iter()
                    .any(|w| w.channel < old.channels.len())
            {
                return Err(
                    "cannot add numeric guards/effects to an already matched channel".into(),
                );
            }
        }
        _ => (),
    }
    if old
        .stages
        .iter()
        .zip(&new.stages)
        .any(|(a, b)| !b.starts_with(a))
    {
        return Err("live install reordered old stages".into());
    }
    Ok(())
}

impl Service {
    pub(crate) fn family_init(&mut self, config: Json) -> Result<Json> {
        write(
            &mut self.input,
            &json!({"op":"family_init","config":config}),
        )?;
        read(&mut self.output)
    }
    pub(crate) fn family_step(&mut self, words: &[u32], ack: usize) -> Result<(Json, Vec<u32>)> {
        write(&mut self.input, &json!({"op":"family_step","ack":ack}))?;
        write_words(&mut self.input, words)?;
        Ok((read(&mut self.output)?, read_words(&mut self.output)?))
    }
    pub(crate) fn table_init(&mut self, config: Json) -> Result<Json> {
        write(&mut self.input, &json!({"op":"table_init","config":config}))?;
        read(&mut self.output)
    }
    pub(crate) fn table_delta(&mut self, words: &[u32], audit: bool) -> Result<(Json, Vec<u32>)> {
        self.table_delta_seeded(words, audit, 0)
    }
    pub(crate) fn table_delta_seeded(
        &mut self,
        words: &[u32],
        audit: bool,
        seeded: usize,
    ) -> Result<(Json, Vec<u32>)> {
        write(
            &mut self.input,
            &json!({"op":"table_delta","audit":audit,"seeded":seeded}),
        )?;
        write_words(&mut self.input, words)?;
        let report = read(&mut self.output)?;
        let data = read_words(&mut self.output)?;
        Ok((report, data))
    }
    pub(crate) fn table_ack(&mut self, epoch: usize) -> Result<()> {
        write(&mut self.input, &json!({"op":"table_ack","epoch":epoch}))?;
        read(&mut self.output)?;
        Ok(())
    }
    pub fn new(worker: &Path) -> Result<Self> {
        let mut child = Command::new(worker)
            .arg("--serve")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut s = Self {
            child,
            input,
            output,
            program: None,
            planner: crate::layout::Planner::default(),
            layout_report: Json::Null,
            install_times: InstallTimes::default(),
        };
        let ready = read(&mut s.output)?;
        if ready["ready"] != true || ready["protocol"] != 9 {
            return Err("live matrix protocol mismatch".into());
        }
        Ok(s)
    }
    pub fn install(&mut self, p: &Program) -> Result<Json> {
        let t = Instant::now();
        if !p.updates.is_empty() {
            return Err("live inputs use immutable event versions, not replay epochs".into());
        }
        if let Some(old) = &self.program {
            extension(old, p)?;
        }
        let checked = p.clone().check()?;
        self.install_times.check_seconds += t.elapsed().as_secs_f64();
        let t = Instant::now();
        let plan = self.planner.plan(&checked)?;
        self.install_times.planner_seconds += t.elapsed().as_secs_f64();
        let t = Instant::now();
        let mut lower = checked.lower_with_layout(&plan)?;
        let data = std::mem::take(&mut lower.data);
        self.layout_report = json!({"plan":plan,"solver_calls":self.planner.solves,"cache_hits":self.planner.cache_hits});
        self.install_times.lower_seconds += t.elapsed().as_secs_f64();
        let t = Instant::now();
        write(
            &mut self.input,
            &json!({"op":"install_binary","program":lower,"word_count":data.len()}),
        )?;
        write_words(&mut self.input, &data)?;
        let r = read(&mut self.output)?;
        let profile = &r["install_profile"];
        self.install_times.worker_decode_seconds +=
            profile["decode_seconds"].as_f64().unwrap_or(0.0);
        self.install_times.worker_buffer_seconds +=
            profile["buffer_seconds"].as_f64().unwrap_or(0.0);
        self.install_times.worker_shader_compile_seconds +=
            profile["shader_compile_seconds"].as_f64().unwrap_or(0.0);
        self.install_times.worker_inherit_seconds +=
            profile["inherit_seconds"].as_f64().unwrap_or(0.0);
        self.program = Some(p.clone());
        self.install_times.worker_install_seconds += t.elapsed().as_secs_f64();
        Ok(r)
    }
    pub fn step(&mut self, resume: bool, disable: &[usize]) -> Result<Json> {
        let p = self.program.as_ref().ok_or("no installed program")?;
        if disable
            .iter()
            .any(|i| p.cells.get(*i).is_none_or(|c| c.binding.is_none()))
        {
            return Err("only original source versions can be retired".into());
        }
        write(
            &mut self.input,
            &json!({"op":if resume{"resume"}else{"advance"},"disable":disable}),
        )?;
        read(&mut self.output)
    }
}
