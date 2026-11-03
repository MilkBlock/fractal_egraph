//! Typed, executable local programs. Endpoints are never used as instructions.
use super::*;

impl Program {
    /// Independent native oracle, used only after device execution in tests.
    pub fn device_reference(&self, unit: usize) -> Result<J> {
        let cell = self.cell(unit)?;
        if cell["ripen_state"] != "Saturated" {
            return Err("native oracle requires an observed finite Cell".into());
        }
        let mut eg = self.execute(unit, false, false)?;
        let anchors = eg.capture_device_anchors().map_err(|e| format!("{e}"))?;
        fn decode(s: &J) -> Result<Schedule> {
            Ok(match text(&s["op"])? {
                "run" if s["until"].is_null() => Schedule::Run(
                    sp(),
                    RunConfig {
                        ruleset: text(&s["ruleset"])?.into(),
                        until: None,
                    },
                ),
                "sequence" => Schedule::Sequence(
                    sp(),
                    arr(&s["body"])?
                        .iter()
                        .map(decode)
                        .collect::<Result<Vec<_>>>()?,
                ),
                "saturate" => Schedule::Saturate(sp(), Box::new(decode(&s["body"])?)),
                "repeat" => {
                    Schedule::Repeat(sp(), num(&s["count"])?, Box::new(decode(&s["body"])?))
                }
                _ => return Err("unsupported reference control".into()),
            })
        }
        let mut events = Vec::new();
        crate::cell_schedule::execute(
            &mut eg,
            &egglog::TraceSession::with_effects(),
            &decode(&cell["closure"])?,
            0,
            &mut |_, e| {
                if e["phase"] == "end" {
                    events.push(e);
                }
                Ok(())
            },
        )?;
        Ok(
            json!({"unit":unit,"checkpoint":eg.device_checkpoint(&anchors).map_err(|e|format!("{e}"))?,"events":events}),
        )
    }
    /// Export a prepared Cell with the *rules*, not the observed terminal rows.
    /// The schedule is the isolated ripen schedule; it grants no tier0 skip.
    pub fn device_program(&self, unit: usize) -> Result<J> {
        let cell = self.cell(unit)?;
        let eg = self.execute(unit, false, false)?;
        fn schedule(s: &J, sets: &mut BTreeSet<String>) -> Result<J> {
            Ok(match text(&s["op"])? {
                "run" => {
                    if !s["until"].is_null() {
                        return Err("local recipe export requires an explicit until probe".into());
                    }
                    let name = text(&s["ruleset"])?;
                    sets.insert(name.to_owned());
                    json!({"run":name})
                }
                "sequence" => {
                    json!({"sequence":arr(&s["body"])?.iter().map(|s|schedule(s,sets)).collect::<Result<Vec<_>>>()?})
                }
                "repeat" => json!({"repeat":num(&s["count"])?,"body":schedule(&s["body"],sets)?}),
                "saturate" => json!({"saturate":schedule(&s["body"],sets)?}),
                _ => return Err("unknown local control operation".into()),
            })
        }
        let mut sets = BTreeSet::new();
        let control = schedule(&cell["closure"], &mut sets)?;
        let mut core = eg
            .export_device_program(&sets.into_iter().collect::<Vec<_>>())
            .map_err(|e| format!("Cell {unit} device export: {e}"))?;
        if let Some(runtime)=cell["native_runtime"].as_str() {
            crate::structural_key::source::attach_device_runtime(&mut core,runtime)?;
        }
        core["schedule"] = json!([control]);
        core["assertion_tables"] = json!([]);
        Ok(core)
    }

    /// One typed environment per distinct program; instances contain only their
    /// initial rows and provenance. Runtime input changes are new instances,
    /// never substitutions into an observed endpoint.
    pub fn device_library(&self, units: &[usize]) -> Result<J> {
        let mut programs = Vec::new();
        let mut keys = BTreeMap::new();
        let mut instances = Vec::new();
        let mut rejected = Vec::new();
        for &unit in units {
            match self.device_program(unit) {
                Ok(mut core) => {
                    let initial = core.as_object_mut().unwrap().remove("initial").unwrap();
                    let key = serde_json::to_string(&core)?;
                    let next = programs.len();
                    let id = *keys.entry(key).or_insert_with(|| {
                        programs.push(core);
                        next
                    });
                    instances.push(json!({"unit":unit,"program":id,"initial":initial,
                        "inputs":self.cell(unit)?["inputs"],"scope":"isolated ripen",
                        "observed_completion":self.cell(unit)?["ripen_state"],
                        "live_substitution_authorized":false}));
                }
                Err(e) => rejected.push(json!({"unit":unit,"reason":e.to_string()})),
            }
        }
        Ok(
            json!({"schema":"analyzer-device-cells/v1","programs":programs,
            "instances":instances,"rejected":rejected,
            "contract":"typed rules, full keys, exact values and complete effects; no endpoint substitution",
            "source_schedule":self.document["source_schedule"]}),
        )
    }
}
