extern crate wpt_interop as interop;
use pyo3::exceptions::PyOSError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::collections::{BTreeMap, BTreeSet};
use std::convert::TryFrom;
use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
struct Error(interop::Error);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::convert::From<interop::Error> for Error {
    fn from(err: interop::Error) -> Error {
        Error(err)
    }
}

impl std::convert::From<Error> for PyErr {
    fn from(err: Error) -> PyErr {
        PyOSError::new_err(err.0.to_string())
    }
}

#[derive(Debug)]
struct Results {
    status: String,
    subtests: Vec<SubtestResult>,
    expected: Option<String>,
}

impl<'source> FromPyObject<'source> for Results {
    // Required method
    fn extract(ob: &'source PyAny) -> PyResult<Results> {
        // Check we get a dictionary
        ob.downcast::<PyDict>()?;
        let status = ob.get_item("status")?.extract()?;
        let subtests = ob.get_item("subtests")?.extract()?;
        let expected = if ob.contains("expected")? {
            Some(ob.get_item("expected")?.extract()?)
        } else {
            None
        };
        Ok(Results {
            status,
            subtests,
            expected,
        })
    }
}

impl IntoPy<PyObject> for Results {
    fn into_py(self, py: Python<'_>) -> PyObject {
        let rv = pyo3::types::PyDict::new(py);
        rv.set_item("status", self.status)
            .expect("Failed to set status");
        rv.set_item(
            "subtests",
            self.subtests
                .into_iter()
                .map(|x| x.into_py(py))
                .collect::<Vec<_>>(),
        )
        .expect("Failed to set subtests");
        rv.into_py(py)
    }
}

impl TryFrom<Results> for interop::Results {
    type Error = interop::Error;

    fn try_from(value: Results) -> Result<interop::Results, interop::Error> {
        Ok(interop::Results {
            status: interop::TestStatus::try_from(value.status.as_ref())?,
            subtests: value
                .subtests
                .iter()
                .map(interop::SubtestResult::try_from)
                .collect::<Result<Vec<_>, _>>()?,
            expected: value
                .expected
                .map(|expected| interop::TestStatus::try_from(expected.as_ref()))
                .transpose()?,
        })
    }
}

impl From<interop::Results> for Results {
    fn from(value: interop::Results) -> Results {
        Results {
            status: value.status.to_string(),
            subtests: value
                .subtests
                .iter()
                .map(SubtestResult::from)
                .collect::<Vec<_>>(),
            expected: value.expected.map(|expected| expected.to_string()),
        }
    }
}

#[derive(Debug)]
struct SubtestResult {
    name: String,
    status: String,
    expected: Option<String>,
}

impl<'source> FromPyObject<'source> for SubtestResult {
    // Required method
    fn extract(ob: &'source PyAny) -> PyResult<SubtestResult> {
        // Check we get a dictionary
        ob.downcast::<PyDict>()?;
        let name = ob.get_item("name")?.extract()?;
        let status = ob.get_item("status")?.extract()?;
        let expected = if ob.contains("expected")? {
            Some(ob.get_item("expected")?.extract()?)
        } else {
            None
        };
        Ok(SubtestResult {
            name,
            status,
            expected,
        })
    }
}

impl IntoPy<PyObject> for SubtestResult {
    fn into_py(self, py: Python<'_>) -> PyObject {
        let rv = pyo3::types::PyDict::new(py);
        rv.set_item("name", self.name).expect("Failed to set id");
        rv.set_item("status", self.status)
            .expect("Failed to set subtest status");
        if self.expected.is_some() {
            rv.set_item("expected", self.expected)
                .expect("Failed to set subtest expected");
        }
        rv.into_py(py)
    }
}

impl TryFrom<&SubtestResult> for interop::SubtestResult {
    type Error = interop::Error;

    fn try_from(value: &SubtestResult) -> Result<interop::SubtestResult, interop::Error> {
        Ok(interop::SubtestResult {
            name: value.name.clone(),
            status: interop::SubtestStatus::try_from(value.status.as_ref())?,
            expected: value
                .expected
                .as_ref()
                .map(|expected| interop::SubtestStatus::try_from(expected.as_ref()))
                .transpose()?,
        })
    }
}

impl From<&interop::SubtestResult> for SubtestResult {
    fn from(value: &interop::SubtestResult) -> SubtestResult {
        SubtestResult {
            name: value.name.clone(),
            status: value.status.to_string(),
            expected: value.expected.map(|expected| expected.to_string()),
        }
    }
}

#[pyfunction]
fn interop_score(
    runs: Vec<BTreeMap<String, Results>>,
    tests: BTreeMap<String, BTreeSet<String>>,
    expected_not_ok: BTreeSet<String>,
) -> PyResult<(
    interop::RunScores,
    interop::InteropScore,
    interop::ExpectedFailureScores,
)> {
    // This is a (second?) copy of all the input data
    let mut interop_runs: Vec<BTreeMap<String, interop::Results>> = Vec::with_capacity(runs.len());
    for run in runs.into_iter() {
        let mut run_map: BTreeMap<String, interop::Results> = BTreeMap::new();
        for (key, value) in run.into_iter() {
            run_map.insert(key, value.try_into().map_err(Error::from)?);
        }
        interop_runs.push(run_map);
    }
    Ok(interop::score_runs(
        interop_runs.iter(),
        &tests,
        &expected_not_ok,
    ))
}

#[pyfunction]
fn run_results(
    results_repo: PathBuf,
    run_ids: Vec<u64>,
    tests: BTreeSet<String>,
) -> PyResult<Vec<BTreeMap<String, Results>>> {
    let results_cache: interop::results_cache::ResultsCache =
        interop::results_cache::ResultsCache::new(&results_repo).map_err(Error::from)?;
    let mut results = Vec::with_capacity(run_ids.len());
    for run_id in run_ids.into_iter() {
        let mut run_results: BTreeMap<String, Results> = BTreeMap::new();
        for (key, value) in results_cache
            .results(run_id, Some(&tests))
            .map_err(Error::from)?
            .into_iter()
        {
            run_results.insert(key, value.into());
        }
        results.push(run_results)
    }
    Ok(results)
}

#[pyfunction]
fn score_runs(
    results_repo: PathBuf,
    run_ids: Vec<u64>,
    tests_by_category: BTreeMap<String, BTreeSet<String>>,
    expected_not_ok: BTreeSet<String>,
) -> PyResult<(
    interop::RunScores,
    interop::InteropScore,
    interop::ExpectedFailureScores,
)> {
    let mut all_tests = BTreeSet::new();
    for tests in tests_by_category.values() {
        all_tests.extend(tests.iter().map(|item| item.into()));
    }
    let results_cache: interop::results_cache::ResultsCache =
        interop::results_cache::ResultsCache::new(&results_repo).map_err(Error::from)?;

    let run_results = run_ids
        .into_iter()
        .map(|run_id| results_cache.results(run_id, Some(&all_tests)))
        .collect::<interop::Result<Vec<_>>>()
        .map_err(Error::from)?;
    Ok(interop::score_runs(
        run_results.iter(),
        &tests_by_category,
        &expected_not_ok,
    ))
}

#[pymodule]
#[pyo3(name = "_wpt_interop")]
fn _wpt_interop(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(interop_score, m)?)?;
    m.add_function(wrap_pyfunction!(run_results, m)?)?;
    m.add_function(wrap_pyfunction!(score_runs, m)?)?;
    Ok(())
}
