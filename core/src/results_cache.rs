use crate::{Error, Result, Results};
use git2;
use serde_json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use urlencoding;

pub struct ResultsCache {
    repo: git2::Repository,
}

impl ResultsCache {
    pub fn new(path: &Path) -> Result<ResultsCache> {
        Ok(ResultsCache {
            repo: git2::Repository::open(path)?,
        })
    }

    fn tag(run_id: u64) -> String {
        format!("refs/tags/run/{}/results", run_id)
    }

    pub fn results(
        &self,
        run_id: u64,
        include_tests: Option<&BTreeSet<String>>,
    ) -> Result<BTreeMap<String, Results>> {
        let mut results_data = BTreeMap::new();
        let run_ref = self.repo.find_reference(&ResultsCache::tag(run_id))?;
        if !run_ref.is_tag() {
            return Err(Error::String(format!(
                "{} is not a tag",
                ResultsCache::tag(run_id)
            )));
        }
        let root = run_ref.peel_to_tree()?;
        let mut stack: Vec<(git2::Tree, String)> = vec![(root, "".to_string())];
        while let Some((tree, path)) = stack.pop() {
            for tree_entry in tree.iter() {
                match tree_entry.kind() {
                    Some(git2::ObjectType::Tree) => {
                        let name = tree_entry.name().ok_or_else(|| {
                            Error::String(format!("Tree has non-utf8 name {:?}", tree_entry.name()))
                        })?;
                        stack.push((
                            tree_entry.to_object(&self.repo)?.peel_to_tree()?,
                            format!("{}/{}", path, name),
                        ));
                    }
                    Some(git2::ObjectType::Blob) => {
                        let name = tree_entry.name().ok_or_else(|| {
                            Error::String(format!("Tree has non-utf8 name {:?}", tree_entry.name()))
                        })?;
                        let test_name = match name.rsplit_once('.') {
                            Some((test_name, "json")) => urlencoding::decode(test_name),
                            Some((_, _)) | None => {
                                return Err(Error::String(format!(
                                    "Expected a name ending .json(), got {}",
                                    name
                                )));
                            }
                        }
                        .expect("Test name is valid utf8");
                        let path = format!("{}/{}", path, test_name);
                        if let Some(include) = include_tests {
                            if !include.contains(&path) {
                                continue;
                            }
                        }
                        let blob = tree_entry.to_object(&self.repo)?.peel_to_blob()?;
                        let results: Results = serde_json::from_slice(blob.content())?;
                        results_data.insert(path, results);
                    }
                    _ => {
                        return Err(Error::String(format!(
                            "Unexpected object while walking tree {}",
                            tree_entry.id()
                        )));
                    }
                }
            }
        }
        Ok(results_data)
    }
}
