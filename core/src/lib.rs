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
struct PassCount {
    passes: Vec<u32>,
    total: Vec<u32>,
}

impl PassCount {
    fn new() -> PassCount {
        PassCount {
            ..Default::default()
        }
    }
}

pub fn score_runs(
    runs: &[BTreeMap<String, Results>],
    interop_tests: &BTreeSet<String>,
    expected_not_ok: &BTreeSet<String>,
) -> Vec<u64> {
    let mut test_pass_counts = BTreeMap::new();
    let mut unexpected_not_ok = BTreeSet::new();
    runs.iter()
        .map(|run| {
            score_run(
                run.iter()
                    .map(|(test_id, results)| (test_id.as_ref(), results)),
                interop_tests,
                expected_not_ok,
                &mut unexpected_not_ok,
                &mut test_pass_counts,
            )
        })
        .collect::<Vec<_>>()
}

fn score_run<'a>(
    run: impl Iterator<Item = (&'a str, &'a Results)>,
    interop_tests: &BTreeSet<String>,
    expected_not_ok: &BTreeSet<String>,
    unexpected_not_ok: &mut BTreeSet<String>,
    pass_counts: &mut BTreeMap<&'a str, PassCount>,
) -> u64 {
    let mut score = 0;
    for (test_id, results) in run {
        if !interop_tests.contains(test_id) {
            continue;
        }
        println!("Scoring {}: status {:?}", test_id, results.status);

        let (run_passes, run_total) = if !results.subtests.is_empty() {
            if results.status != Status::Ok && !expected_not_ok.contains(test_id) {
                unexpected_not_ok.insert(test_id.into());
            }
            (
                results
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
                results.subtests.len() as u32,
            )
        } else {
            if results.status == Status::Pass {
                (1, 1)
            } else {
                (0, 1)
            }
        };
        println!(
            "{}: run passes {}, run total {}",
            test_id, run_passes, run_total
        );
        let pass_count = pass_counts.entry(test_id).or_insert_with(PassCount::new);
        pass_count.passes.push(run_passes);
        pass_count.total.push(run_total);
        score += (1000. * run_passes as f64 / run_total as f64).trunc() as u64;
    }
    score / interop_tests.len() as u64
}
