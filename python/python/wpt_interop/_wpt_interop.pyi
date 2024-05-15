from typing import Mapping, Optional

RunScores = Mapping[str, list[int]]
InteropScore = Mapping[str, int]
ExpectedFailureScores = Mapping[str, list[tuple[int, int]]]


class Results:
    status: str
    subtests: list[SubtestResult]
    expected: Optional[str]


class SubtestResult:
    name: str
    status: str
    expected: Optional[str]


def interop_score(runs: list[Mapping[str, Results]],
                  tests: Mapping[str, set[str]],
                  expected_not_ok: set[str]) -> tuple[RunScores,
                                                      InteropScore,
                                                      ExpectedFailureScores]:
    ...


def run_results(results_repo: str,
                run_ids: list[int],
                tests: set[str]) -> list[Mapping[str, Results]]:
    ...


def score_runs(results_repo: str,
               run_ids: list[int],
               tests_by_category: Mapping[str, set[str]],
               expected_not_ok: set[str]) -> tuple[RunScores,
                                                   InteropScore,
                                                   ExpectedFailureScores]:
    ...


def interop_tests(metadata_repo_path: str,
                  labels_by_category: Mapping[str, set[str]],
                  metadata_revision: Optional[str]) -> tuple[str,
                                                             Mapping[str, set[str]], set[str]]:
    ...


def regressions(results_repo: str,
                metadata_repo_path: str,
                run_ids: tuple[int, int]) -> Mapping[str, tuple[Optional[str],
                                                                list[tuple[str, str]],
                                                                list[str]]]:
    ...
