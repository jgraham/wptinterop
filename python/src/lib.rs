extern crate interop_score as interop;
use pyo3::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(FromPyObject)]
struct Results {
    #[pyo3(item)]
    status: String,
    #[pyo3(item)]
    subtests: Vec<SubtestResult>,
}

impl From<&Results> for interop::Results {
    fn from(value: &Results) -> interop::Results {
        interop::Results {
            status: interop::Status::from(value.status.as_ref()),
            subtests: value
                .subtests
                .iter()
                .map(interop::SubtestResult::from)
                .collect::<Vec<_>>(),
        }
    }
}

#[derive(FromPyObject)]
struct SubtestResult {
    #[pyo3(item)]
    id: String,
    #[pyo3(item)]
    status: String,
}

impl From<&SubtestResult> for interop::SubtestResult {
    fn from(value: &SubtestResult) -> interop::SubtestResult {
        interop::SubtestResult {
            id: value.id.clone(),
            status: interop::Status::from(value.status.as_ref()),
        }
    }
}

#[pyfunction]
fn score_runs(
    runs: Vec<BTreeMap<String, Results>>,
    tests: BTreeSet<String>,
    expected_not_ok: BTreeSet<String>,
) -> PyResult<Vec<u64>> {
    // This is a (second?) copy of all the input data
    let mut interop_runs: Vec<BTreeMap<String, interop::Results>> = Vec::with_capacity(runs.len());
    for run in runs.iter() {
        let mut run_map: BTreeMap<String, interop::Results> = BTreeMap::new();
        for (key, value) in run.iter() {
            run_map.insert(key.clone(), value.into());
        }
        interop_runs.push(run_map);
    }
    Ok(interop::score_runs(&interop_runs, &tests, &expected_not_ok))
}

#[pymodule]
#[pyo3(name = "_interop_score")]
fn _interop_score(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(score_runs, m)?)?;
    Ok(())
}
