//! Deterministic native mod ordering using optional mod.json dependencies.

use std::{collections::HashSet, fs, io::Read, path::PathBuf};

pub struct Candidate {
    pub path: PathBuf,
    pub id: Option<String>,
    pub dependencies: Vec<String>,
}

pub fn inspect(path: PathBuf) -> Result<Candidate, String> {
    let mut id = None;
    let mut dependencies = Vec::new();
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("mmc"))
    {
        let file = fs::File::open(&path).map_err(|e| e.to_string())?;
        let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
        let mut text = String::new();
        zip.by_name("mod.json")
            .map_err(|_| "missing mod.json".to_string())?
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
        let manifest: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("invalid mod.json: {e}"))?;
        let declared = manifest["id"]
            .as_str()
            .filter(|name| crate::valid_namespace(name))
            .ok_or("missing or invalid mod.json id")?;
        id = Some(declared.to_owned());
        if let Some(deps) = manifest.get("dependencies") {
            let items = deps.as_array().ok_or("dependencies must be an array")?;
            if items.len() > 32 {
                return Err("too many mod dependencies".to_string());
            }
            let mut unique = HashSet::new();
            for dep in items {
                let name = dep
                    .as_str()
                    .filter(|name| crate::valid_namespace(name))
                    .ok_or("dependency must be a mod id")?;
                if name == declared || !unique.insert(name) {
                    return Err("self or duplicate mod dependency".to_string());
                }
                dependencies.push(name.to_owned());
            }
        }
    }
    Ok(Candidate {
        path,
        id,
        dependencies,
    })
}

/// Pick a candidate only when all its declared providers completed successful
/// startup and registration. Stable among equally ready candidates.
pub fn next_ready(pending: &[Candidate], completed: &HashSet<String>) -> Option<usize> {
    pending
        .iter()
        .position(|c| c.dependencies.iter().all(|d| completed.contains(d)))
}

#[cfg(test)]
mod tests {
    use super::{Candidate, next_ready};
    use std::{collections::HashSet, path::PathBuf};

    fn candidate(id: &str, deps: &[&str]) -> Candidate {
        Candidate {
            path: PathBuf::from(id),
            id: Some(id.to_owned()),
            dependencies: deps.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn provider_precedes_lexically_earlier_consumer() {
        let items = vec![candidate("addon", &["create"]), candidate("create", &[])];
        assert_eq!(next_ready(&items, &HashSet::new()), Some(1));
        assert_eq!(
            next_ready(&items, &HashSet::from(["create".to_owned()])),
            Some(0)
        );
    }

    #[test]
    fn missing_or_cyclic_dependencies_do_not_start() {
        let missing = vec![candidate("addon", &["absent"])];
        assert_eq!(next_ready(&missing, &HashSet::new()), None);
        let cycle = vec![candidate("one", &["two"]), candidate("two", &["one"])];
        assert_eq!(next_ready(&cycle, &HashSet::new()), None);
    }
}
