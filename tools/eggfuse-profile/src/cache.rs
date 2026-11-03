use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn hash_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).with_context(|| format!("read {}", path.display()))?;
    let mut hash = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub fn read_json(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(
        &fs::read(path).with_context(|| format!("read {}", path.display()))?,
    )?)
}
pub fn write_json(path: &Path, value: &Value) -> Result<()> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    let temp = path.with_extension("pending");
    fs::write(&temp, serde_json::to_vec_pretty(value)?)?;
    fs::rename(temp, path)?;
    Ok(())
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

pub fn files_under(root: &Path, extensions: &[&str]) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            if !["__pycache__", "target", ".git"]
                .contains(&entry.file_name().to_string_lossy().as_ref())
            {
                out.extend(files_under(&path, extensions)?);
            }
        } else if (kind.is_file() || (kind.is_symlink() && path.is_file()))
            && (extensions.is_empty()
                || path
                    .extension()
                    .and_then(|x| x.to_str())
                    .is_some_and(|x| extensions.contains(&x)))
        {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}
pub fn fingerprints(paths: impl IntoIterator<Item = PathBuf>) -> Result<Value> {
    let mut out = serde_json::Map::new();
    for path in paths {
        out.insert(
            path.to_string_lossy().into_owned(),
            json!(hash_file(&path)?),
        );
    }
    Ok(Value::Object(out))
}

pub struct Cache {
    pub root: PathBuf,
    pub only: bool,
    pub plan: bool,
}
pub struct Entry {
    pub dir: PathBuf,
    pub detail: Value,
    pub hit: bool,
    pub key: String,
    pub created: u64,
}
impl Cache {
    pub fn stage(
        &self,
        name: &str,
        inputs: Value,
        fresh: bool,
        action: impl FnOnce(&Path) -> Result<(Value, Vec<PathBuf>)>,
    ) -> Result<Entry> {
        let key = hash_bytes(&serde_json::to_vec(&inputs)?);
        let base = self.root.join(name).join(&key);
        let receipt = base.join("receipt.json");
        if !fresh
            && receipt.exists()
            && let Ok(r) = read_json(&receipt)
        {
            if valid_receipt(&r, &inputs)? {
                println!(
                    "  HIT  {name}  {} (saved {})",
                    &key[..10],
                    r["created_unix"]
                );
                return Ok(Entry {
                    dir: PathBuf::from(r["directory"].as_str().context("receipt directory")?),
                    detail: r["detail"].clone(),
                    hit: true,
                    key,
                    created: r["created_unix"].as_u64().unwrap_or(0),
                });
            }
            println!("  STALE {name}: incomplete or changed outputs");
        }
        if self.only || self.plan {
            bail!("MISS {name} {} (no execution requested)", &key[..10]);
        }
        fs::create_dir_all(&base)?;
        let mut i = 0;
        while base.join(format!("attempt-{i}")).exists() {
            i += 1;
        }
        let dir = base.join(format!("attempt-{i}"));
        fs::create_dir(&dir)?;
        println!("  RUN  {name}  {}", &key[..10]);
        write_json(&dir.join("inputs.json"), &inputs)?;
        let (detail, outputs) = match action(&dir) {
            Ok(v) => v,
            Err(e) => {
                write_json(
                    &dir.join("failure.json"),
                    &json!({"error":format!("{e:#}"),"created_unix":now()}),
                )?;
                return Err(e.context(format!("{name}; diagnostics: {}", dir.display())));
            }
        };
        ensure!(!outputs.is_empty(), "stage must declare retained evidence");
        let created = now();
        write_json(
            &receipt,
            &json!({"schema":"eggfuse-profile-cache/v1","validated":true,"inputs":inputs,"outputs":fingerprints(outputs)?,"directory":dir,"detail":detail,"created_unix":created}),
        )?;
        Ok(Entry {
            dir,
            detail,
            hit: false,
            key,
            created,
        })
    }
}
pub fn valid_receipt(r: &Value, inputs: &Value) -> Result<bool> {
    if r["schema"] != "eggfuse-profile-cache/v1" || r["validated"] != true || &r["inputs"] != inputs
    {
        return Ok(false);
    }
    let Some(outputs) = r["outputs"].as_object().filter(|o| !o.is_empty()) else {
        return Ok(false);
    };
    for (p, h) in outputs {
        if hash_file(Path::new(p)).ok().as_deref() != h.as_str() {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn content_cache_rejects_corruption_and_changed_inputs() -> Result<()> {
        let root =
            std::env::temp_dir().join(format!("eggfuse-profile-cache-{}", std::process::id()));
        fs::create_dir_all(&root)?;
        let file = root.join("result.json");
        fs::write(&file, "ok")?;
        let inputs = json!({"program":"a","worker":"b","device":"c"});
        let r = json!({"schema":"eggfuse-profile-cache/v1","validated":true,"inputs":inputs,"outputs":fingerprints([file.clone()])?});
        assert!(valid_receipt(&r, &inputs)?);
        assert!(!valid_receipt(&r, &json!({"program":"other"}))?);
        fs::write(&file, "bad")?;
        assert!(!valid_receipt(&r, &inputs)?);
        fs::remove_dir_all(root)?;
        Ok(())
    }
    #[test]
    fn sha_is_standard() {
        assert_eq!(
            hash_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
    #[test]
    fn cache_hit_fresh_and_failed_attempts_have_distinct_lifetimes() -> Result<()> {
        let root =
            std::env::temp_dir().join(format!("eggfuse-profile-lifecycle-{}", std::process::id()));
        let c = Cache {
            root: root.clone(),
            only: false,
            plan: false,
        };
        let input = json!({"source":"fixture"});
        let action = |d: &Path| {
            let p = d.join("ok");
            fs::write(&p, "evidence")?;
            Ok((json!({"value":1}), vec![p]))
        };
        let first = c.stage("run", input.clone(), false, action)?;
        assert!(!first.hit);
        let second = c.stage("run", input.clone(), false, |_| {
            panic!("cache must not execute")
        })?;
        assert!(second.hit);
        assert_eq!(first.dir, second.dir);
        let fresh = c.stage("run", input.clone(), true, action)?;
        assert!(!fresh.hit);
        assert_ne!(fresh.dir, first.dir);
        assert!(first.dir.join("ok").exists());
        fs::write(fresh.dir.join("ok"), "corrupt")?;
        let strict = Cache {
            root: root.clone(),
            only: true,
            plan: false,
        };
        assert!(
            strict
                .stage("run", input, false, |_| panic!(
                    "cache-only must not execute"
                ))
                .is_err()
        );
        assert!(
            c.stage("failed", json!({}), false, |_| bail!("fixture failure"))
                .is_err()
        );
        let recovered = c.stage("failed", json!({}), false, action)?;
        assert!(!recovered.hit);
        assert!(recovered.dir.ends_with("attempt-1"));
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
