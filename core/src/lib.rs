use std::collections::{BTreeMap, BTreeSet};
use std::default::Default;

#[derive(Debug)]
pub struct Results {
    pub status: Status,
    pub subtests: Vec<SubtestResult>,
}

#[derive(Debug)]
pub struct SubtestResult {
    pub id: String,
    pub status: Status,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Status {
    Ok,
    Pass,
    Other,
}

impl From<&str> for Status {
    fn from(value: &str) -> Self {
        match value {
            "PASS" => Status::Pass,
            "OK" => Status::Ok,
            _ => Status::Other,
        }
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
    category_scores: Vec<u64>,
    unexpected_not_ok: BTreeSet<String>,
}

impl RunScore {
    fn new(size: usize) -> RunScore {
        RunScore {
            category_scores: vec![0; size],
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
            let (test_passes, test_total) = if !test_results.subtests.is_empty() {
                if test_results.status != Status::Ok && !expected_not_ok.contains(test_id) {
                    run_score.unexpected_not_ok.insert(test_id.into());
                }
                (
                    test_results
                        .subtests
                        .iter()
                        .map(|subtest| {
                            if (subtest.status) == Status::Pass {
                                1
                            } else {
                                0
                            }
                        })
                        .sum(),
                    test_results.subtests.len() as u32,
                )
            } else {
                if test_results.status == Status::Pass {
                    (1, 1)
                } else {
                    (0, 1)
                }
            };
            for category_idx in categories {
                let test_scores = &mut test_scores_by_category[*category_idx];
                let pass_count = test_scores.entry(test_id).or_insert_with(Vec::new);
                pass_count.push(TestScore::new(test_passes, test_total as u64));

                run_score.category_scores[*category_idx] +=
                    (1000. * test_passes as f64 / test_total as f64).trunc() as u64;
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
    for test_score in test_scores {
        if test_score.len() != num_runs {
            continue;
        }
        let min_score = test_score
            .iter()
            .map(|score| (1000. * (score.passes as f64 / score.total as f64).trunc()) as u64)
            .min()
            .unwrap_or(0);
        interop_score += min_score
    }
    (interop_score as f64 / num_runs as f64).trunc() as u64
}

/// Compute the Interop scores for a set of web-platform-tests runs
///
/// * `runs` - One element for each run, containing a mapping from test id to test results.
/// * `tests_by_category` - Mapping from category to the set of test ids in that category
/// * `expected_not_ok` - Set of tests which are known to have non-OK statuses
///
/// Returns a tuple of
/// (Mapping from category to score per run, Mapping of category to interop score for all runs)
pub fn score_runs(
    runs: &[BTreeMap<String, Results>],
    tests_by_category: &BTreeMap<String, BTreeSet<String>>,
    expected_not_ok: &BTreeSet<String>,
) -> (BTreeMap<String, Vec<u64>>, BTreeMap<String, u64>) {
    let mut unexpected_not_ok = BTreeSet::new();

    // Instead of passing round per-category maps, use a vector with categories at a fixed index
    let num_categories = tests_by_category.len();
    let mut categories = Vec::with_capacity(num_categories);
    let mut test_count_by_category = Vec::with_capacity(num_categories);
    let mut test_scores_by_category = Vec::with_capacity(num_categories);

    let mut categories_by_test = BTreeMap::new();

    let mut scores_by_category = BTreeMap::new();
    let mut interop_by_category = BTreeMap::new();

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
        scores_by_category.insert(category.clone(), Vec::with_capacity(runs.len()));
        interop_by_category.insert(category.clone(), 0);
    }

    for run in runs {
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
                .push(run_score.category_scores[idx] / test_count_by_category[idx] as u64)
        }
        unexpected_not_ok.extend(run_score.unexpected_not_ok)
    }
    for (idx, name) in categories.iter().enumerate() {
        let scores = &test_scores_by_category[idx];
        interop_by_category.insert((*name).clone(), interop_score(scores.values(), runs.len()));
    }
    (scores_by_category, interop_by_category)
}
