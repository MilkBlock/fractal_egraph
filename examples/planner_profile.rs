//! SOURCE.egg OUTPUT.jsonl; stdin/stdout piping is supported with output '-'.
use cell_assisted_egglog::{parameterized_cell::profile::Profiler,Result};
use egglog::{EGraph,ast::Command};
use std::io::Write;
fn main()->Result<()> {
    let a=std::env::args().skip(1).collect::<Vec<_>>();if a.len()!=2{return Err("SOURCE OUTPUT".into())}
    EGraph::set_num_threads(1);egglog::set_probe_counts_enabled(false);
    let mut eg=EGraph::default();let source=std::fs::read_to_string(&a[0])?;
    cell_assisted_egglog::stable_choice::source::install(&mut eg,&source)?;
    let commands=eg.parse_program(Some(a[0].clone()),&source)?;
    let mut out:Box<dyn Write>=if a[1]=="-"{Box::new(std::io::BufWriter::new(std::io::stdout()))}else{Box::new(std::io::BufWriter::new(std::fs::File::create(&a[1])?))};
    let mut profiler=None;let mut pending=vec![];let mut epoch=0;
    for(i,c)in commands.iter().enumerate(){
        if let Command::RunSchedule(s)=c {
            if profiler.is_none(){
                eg.run_program(std::mem::take(&mut pending))?;
                let p=Profiler::new(std::mem::take(&mut eg),&commands[..i])?;
                serde_json::to_writer(&mut out,&serde_json::json!({"epoch":epoch,"header":p.header}))?;writeln!(out)?;
                profiler=Some(p);
            }
            profiler.as_mut().unwrap().schedule(s,&i.to_string(),&mut out)?;
        }else if matches!(c,Command::Action(..)|Command::Check(..)|Command::Fail(..)) && profiler.is_some() {
            profiler.as_mut().unwrap().input(vec![c.clone()])?;
        }else{
            if let Some(p)=profiler.take(){eg=p.into_engine();epoch+=1;}
            pending.push(c.clone());
        }
    }
    if let Some(p)=profiler{eg=p.into_engine();}
    eg.run_program(pending)?;
    serde_json::to_writer(&mut out,&serde_json::json!({"complete":true,"epochs":epoch+1}))?;writeln!(out)?;out.flush()?;Ok(())
}
