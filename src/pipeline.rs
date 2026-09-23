pub type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
/// A cancelled run leaves the last complete checkpoint, not a truncated JSON file.
pub(crate) fn write_json(path: &std::path::Path, value: &impl serde::Serialize) -> Result {
    use std::io::Write;
    let temp = path.with_extension("json.partial");
    let mut writer = std::io::BufWriter::new(std::fs::File::create(&temp)?);
    serde_json::to_writer(&mut writer, value)?;
    writer.flush()?;
    drop(writer);
    std::fs::rename(temp, path)?;
    Ok(())
}
