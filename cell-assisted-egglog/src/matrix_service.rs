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
fn prefix(a: &Json, b: &Json) -> bool {
    a.as_array()
        .zip(b.as_array())
        .is_some_and(|(a, b)| b.starts_with(a))
}
fn extension(old: &Program, new: &Program) -> Result<()> {
    let a = serde_json::to_value(old)?;
    let b = serde_json::to_value(new)?;
    if old.bits != new.bits || new.slots < old.slots || !new.updates.is_empty() {
        return Err(
            "live install cannot change coordinate semantics; retire source versions instead"
                .into(),
        );
    }
    for field in ["nodes", "cells", "templates", "conditions"] {
        if !prefix(&a[field], &b[field]) {
            return Err(format!("live install changed existing {field}").into());
        }
    }
    if new.channels.len() < old.channels.len() || new.stages.len() < old.stages.len() {
        return Err("live program cannot shrink".into());
    }
    for (i, old) in a["channels"].as_array().unwrap().iter().enumerate() {
        let mut expected = old.clone();
        let mut actual = b["channels"][i].clone();
        if !old["target"].is_null() {
            let os = old["target"]
                .get("specializations")
                .cloned()
                .unwrap_or(json!([]));
            let ns = actual["target"]
                .get("specializations")
                .cloned()
                .unwrap_or(json!([]));
            if !prefix(&os, &ns) {
                return Err("cannot replace a specialization certificate".into());
            }
            expected["target"]
                .as_object_mut()
                .unwrap()
                .remove("specializations");
            actual["target"]
                .as_object_mut()
                .unwrap()
                .remove("specializations");
        }
        if expected != actual {
            return Err("live install changed a completed match or its binding".into());
        }
    }
    match (&old.numeric, &new.numeric) {
        (Some(_), None) => return Err("cannot remove numeric semantics".into()),
        (a, Some(n)) => {
            if a.is_some() {
                for field in [
                    "inputs",
                    "state_bindings",
                    "exprs",
                    "guards",
                    "groups",
                    "writes",
                ] {
                    if !prefix(
                        &serde_json::to_value(a.as_ref().unwrap())?[field],
                        &serde_json::to_value(n)?[field],
                    ) {
                        return Err(format!("live install changed numeric {field}").into());
                    }
                }
            }
            let guards = a.as_ref().map_or(0, |a| a.guards.len());
            let writes = a.as_ref().map_or(0, |a| a.writes.len());
            if n.guards[guards..]
                .iter()
                .any(|g| g.channel < old.channels.len())
                || n.writes[writes..]
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
    for (a, b) in old.stages.iter().zip(&new.stages) {
        if !b.starts_with(a) {
            return Err("live install reordered old stages".into());
        }
    }
    Ok(())
}
impl Service {
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
        if ready["ready"] != true || ready["protocol"] != 5 {
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
        let lower = checked.lower_with_layout(&plan)?;
        self.layout_report = json!({"plan":plan,"solver_calls":self.planner.solves,"cache_hits":self.planner.cache_hits});
        self.install_times.lower_seconds += t.elapsed().as_secs_f64();
        let t = Instant::now();
        write(&mut self.input, &json!({"op":"install","program":lower}))?;
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
