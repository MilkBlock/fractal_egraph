use egg_layout::source_ast::export;
use std::{error::Error, fs, io::Write};
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 3 {
        return Err("usage: lhs_ast SOURCE.egg OUTPUT.json".into());
    }
    let source = fs::read_to_string(&args[1])?;
    let report = export(&args[1].to_string_lossy(), &source)?;
    let mut output = std::io::BufWriter::new(fs::File::create(&args[2])?);
    serde_json::to_writer_pretty(&mut output, &report)?;
    writeln!(output)?;
    output.flush()?;
    Ok(())
}
