pub mod metadata;
pub mod results_cache;

use serde_derive::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::default::Default;
use std::fmt::Display;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;
pub type RunScores = BTreeMap<String, Vec<u64>>;
pub type InteropScore = BTreeMap<String, u64>;
pub type ExpectedFailureScores = BTreeMap<String, Vec<(u64, u64)>>;

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Git(#[from] git2::Error),
    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),
    #[error(transparent)]
    SerdeYaml(#[from] serde_yaml::Error),
    #[error("{0}")]
    String(String),
}

#[derive(Debug, Deserialize)]
pub struct Results {
    pub status: TestStatus,
    #[serde(default)]
    pub subtests: Vec<SubtestResult>,
    #[serde(default)]
    pub expected: Option<TestStatus>,
}

#[derive(Debug, Deserialize)]
pub struct SubtestResult {
    pub name: String,
    pub status: SubtestStatus,
    #[serde(default)]
    pub expected: Option<SubtestStatus>,
}

#[derive(Deserialize, PartialEq, Eq, Clone, Debug, Copy, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TestStatus {
    Pass,
    Fail,
    Ok,
    Error,
    Timeout,
    Crash,
    Assert,
    PreconditionFailed,
    Skip,
}

impl TryFrom<&str> for TestStatus {
    type Error = Error;

    fn try_from(value: &str) -> Result<TestStatus> {
        match value {
            "PASS" => Ok(TestStatus::Pass),
            "FAIL" => Ok(TestStatus::Fail),
            "OK" => Ok(TestStatus::Ok),
            "ERROR" => Ok(TestStatus::Error),
            "TIMEOUT" => Ok(TestStatus::Timeout),
            "CRASH" => Ok(TestStatus::Crash),
            "ASSERT" => Ok(TestStatus::Assert),
            "PRECONDITION_FAILED" => Ok(TestStatus::PreconditionFailed),
            "SKIP" => Ok(TestStatus::Skip),
            x => Err(Error::String(format!("Unrecognised test status {}", x))),
        }
    }
}

impl Display for TestStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                TestStatus::Pass => "PASS",
                TestStatus::Fail => "FAIL",
                TestStatus::Ok => "OK",
                TestStatus::Error => "ERROR",
                TestStatus::Timeout => "TIMEOUT",
                TestStatus::Crash => "CRASH",
                TestStatus::Assert => "ASSERT",
                TestStatus::PreconditionFailed => "PRECONDITION_FAILED",
                TestStatus::Skip => "SKIP",
            }
        )
    }
}

#[derive(Deserialize, PartialEq, Eq, Clone, Debug, Copy, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SubtestStatus {
    Pass,
    Fail,
    Error,
    Timeout,
    Assert,
    PreconditionFailed,
    Notrun,
    Skip,
}

impl TryFrom<&str> for SubtestStatus {
    type Error = Error;

    fn try_from(value: &str) -> Result<SubtestStatus> {
        match value {
            "PASS" => Ok(SubtestStatus::Pass),
            "FAIL" => Ok(SubtestStatus::Fail),
            "ERROR" => Ok(SubtestStatus::Error),
            "TIMEOUT" => Ok(SubtestStatus::Timeout),
            "ASSERT" => Ok(SubtestStatus::Assert),
            "PRECONDITION_FAILED" => Ok(SubtestStatus::PreconditionFailed),
            "NOTRUN" => Ok(SubtestStatus::Notrun),
            "SKIP" => Ok(SubtestStatus::Skip),
            x => Err(Error::String(format!("Unrecognised subtest status {}", x))),
        }
    }
}

impl Display for SubtestStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                SubtestStatus::Pass => "PASS",
                SubtestStatus::Fail => "FAIL",
                SubtestStatus::Error => "ERROR",
                SubtestStatus::Timeout => "TIMEOUT",
                SubtestStatus::Assert => "ASSERT",
                SubtestStatus::PreconditionFailed => "PRECONDITION_FAILED",
                SubtestStatus::Notrun => "NOTRUN",
                SubtestStatus::Skip => "SKIP",
            }
        )
    }
}

#[derive(Debug, Default)]
struct TestScore {
    passes: u64,
    total: u64,
}

impl TestScore {
    fn new(passes: u64, total: u64) -> TestScore {
        TestScore { passes, total }
    }
}

#[derive(Debug, Default)]
struct RunScore {
    category_scores: Vec<f64>,
    category_expected_failures: Vec<f64>,
    unexpected_not_ok: BTreeSet<String>,
}

impl RunScore {
    fn new(size: usize) -> RunScore {
        RunScore {
            category_scores: vec![0.; size],
            category_expected_failures: vec![0.; size],
            ..Default::default()
        }
    }
}

fn score_run<'a>(
    run: impl Iterator<Item = (&'a str, &'a Results)>,
    num_categories: usize,
    categories_by_test: &BTreeMap<&'a str, Vec<usize>>,
    expected_not_ok: &BTreeSet<String>,
    test_scores_by_category: &mut [BTreeMap<&'a str, Vec<TestScore>>],
) -> RunScore {
    let mut run_score = RunScore::new(num_categories);
    for (test_id, test_results) in run {
        if let Some(categories) = categories_by_test.get(test_id) {
            if test_results.status != TestStatus::Ok && !expected_not_ok.contains(test_id) {
                run_score.unexpected_not_ok.insert(test_id.into());
            }

            let (test_passes, expected_failures, test_total) = if !test_results.subtests.is_empty()
            {
                let (test_passes, expected_failures) = test_results
                    .subtests
                    .iter()
                    .map(|subtest| {
                        if (subtest.status) == SubtestStatus::Pass {
                            (1, 0)
                        } else {
                            (
                                0,
                                if (test_results.expected.is_some()
                                    && test_results.expected != Some(TestStatus::Ok)
                                    && test_results.expected != Some(TestStatus::Pass))
                                    || (subtest.expected.is_some()
                                        && subtest.expected != Some(SubtestStatus::Pass))
                                {
                                    1
                                } else {
                                    0
                                },
                            )
                        }
                    })
                    .fold((0, 0), |acc, elem| (acc.0 + elem.0, acc.1 + elem.1));
                (
                    test_passes,
                    expected_failures,
                    test_results.subtests.len() as u32,
                )
            } else {
                let (is_pass, expected_failure) = if test_results.status == TestStatus::Pass {
                    (1, 0)
                } else {
                    (
                        0,
                        if test_results.expected.is_some()
                            && test_results.expected != Some(TestStatus::Ok)
                            && test_results.expected != Some(TestStatus::Pass)
                        {
                            1
                        } else {
                            0
                        },
                    )
                };
                (is_pass, expected_failure, 1)
            };
            for category_idx in categories {
                let test_scores = &mut test_scores_by_category[*category_idx];
                let pass_count = test_scores.entry(test_id).or_default();
                pass_count.push(TestScore::new(test_passes, test_total as u64));

                run_score.category_scores[*category_idx] += test_passes as f64 / test_total as f64;
                run_score.category_expected_failures[*category_idx] +=
                    expected_failures as f64 / test_total as f64;
            }
        }
    }
    run_score
}

fn interop_score<'a>(
    test_scores: impl Iterator<Item = &'a Vec<TestScore>>,
    num_runs: usize,
) -> u64 {
    let mut interop_score = 0;
    let mut num_test_scores = 0;
    for test_score in test_scores {
        num_test_scores += 1;
        if test_score.len() != num_runs {
            continue;
        }
        let min_score = test_score
            .iter()
            .map(|score| (1000. * score.passes as f64 / score.total as f64).trunc() as u64)
            .min()
            .unwrap_or(0);
        interop_score += min_score
    }
    (interop_score as f64 / num_test_scores as f64).trunc() as u64
}

/// Compute the Interop scores for a set of web-platform-tests runs
///
/// * `runs` - One element for each run, containing a mapping from test id to test results.
/// * `tests_by_category` - Mapping from category to the set of test ids in that category
/// * `expected_not_ok` - Set of tests which are known to have non-OK statuses
///
/// Returns a tuple of
/// (Mapping from category to score per run,
///  Mapping of category to interop score for all runs,
///  Mapping of category to expected failure score for each run)
pub fn score_runs<'a>(
    runs: impl Iterator<Item = &'a BTreeMap<String, Results>>,
    tests_by_category: &BTreeMap<String, BTreeSet<String>>,
    expected_not_ok: &BTreeSet<String>,
) -> (RunScores, InteropScore, ExpectedFailureScores) {
    let mut unexpected_not_ok = BTreeSet::new();

    // Instead of passing round per-category maps, use a vector with categories at a fixed index
    let num_categories = tests_by_category.len();
    let mut categories = Vec::with_capacity(num_categories);
    let mut test_count_by_category = Vec::with_capacity(num_categories);
    let mut test_scores_by_category = Vec::with_capacity(num_categories);

    let mut categories_by_test = BTreeMap::new();

    let mut scores_by_category = BTreeMap::new();
    let mut interop_by_category = BTreeMap::new();
    let mut expected_failures_by_category = BTreeMap::new();

    for (cat_idx, (category, tests)) in tests_by_category.iter().enumerate() {
        categories.push(category);
        test_count_by_category.push(tests.len());
        test_scores_by_category.push(BTreeMap::new());

        for test_id in tests {
            categories_by_test
                .entry(test_id.as_ref())
                .or_insert_with(Vec::new)
                .push(cat_idx)
        }
        scores_by_category.insert(category.clone(), Vec::with_capacity(runs.size_hint().0));
        expected_failures_by_category
            .insert(category.clone(), Vec::with_capacity(runs.size_hint().0));
        interop_by_category.insert(category.clone(), 0);
    }

    let mut run_count = 0;
    for run in runs {
        run_count += 1;
        let run_score = score_run(
            run.iter()
                .map(|(test_id, results)| (test_id.as_ref(), results)),
            num_categories,
            &categories_by_test,
            expected_not_ok,
            &mut test_scores_by_category,
        );
        for (idx, name) in categories.iter().enumerate() {
            scores_by_category
                .get_mut(*name)
                .expect("Missing category")
                .push(
                    (1000. * run_score.category_scores[idx] / test_count_by_category[idx] as f64)
                        .trunc() as u64,
                );
            expected_failures_by_category
                .get_mut(*name)
                .expect("Missing category")
                .push((
                    (1000. * run_score.category_expected_failures[idx]
                        / test_count_by_category[idx] as f64)
                        .trunc() as u64,
                    (1000.
                        * (run_score.category_scores[idx]
                            / (test_count_by_category[idx] as f64
                                - run_score.category_expected_failures[idx])))
                        .trunc() as u64,
                ));
        }
        unexpected_not_ok.extend(run_score.unexpected_not_ok)
    }
    for (idx, name) in categories.iter().enumerate() {
        let scores = &test_scores_by_category[idx];
        interop_by_category.insert((*name).clone(), interop_score(scores.values(), run_count));
    }
    (
        scores_by_category,
        interop_by_category,
        expected_failures_by_category,
    )
}
