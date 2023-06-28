import gzip
import json
import os
from collections import defaultdict
from typing import Any, Callable, Iterable, List, Mapping, Optional, Set

import requests

from . import _wpt_interop  # type: ignore

CATEGORY_URL = "https://raw.githubusercontent.com/web-platform-tests/results-analysis/main/interop-scoring/category-data.json"
INTEROP_DATA_URL = "https://wpt.fyi/static/interop-data.json"
METADATA_URL = "https://wpt.fyi/api/metadata?includeTestLevel=true&product=chrome"


def fetch_category_data() -> Mapping[str, Mapping[str, Any]]:
    return requests.get(CATEGORY_URL).json()


def fetch_interop_data() -> Mapping[str, Mapping[str, Any]]:
    return requests.get(INTEROP_DATA_URL).json()


def fetch_labelled_tests() -> Mapping[str, set]:
    rv = defaultdict(set)
    data = requests.get(METADATA_URL).json()
    for test, metadata in data.items():
        for meta_item in metadata:
            if "label" in meta_item:
                rv[meta_item["label"]].add(test)
    return rv


def is_gzip(path):
    if os.path.splitext(path) == ".gz":
        return True
    try:
        # Check for magic number at the start of the file
        with open(path, "rb") as f:
            return f.read(2) == b"\x1f\x8b"
    except Exception:
        return False


def categories_for_year(year: int,
                        category_data: Mapping[str, Mapping[str, Any]],
                        interop_data: Mapping[str, Mapping[str, Any]]) -> List[Mapping[str, Any]]:
    year_key = str(year)
    if year_key not in category_data or year_key not in interop_data:
        raise ValueError(f"Invalid year {year}")
    all_categories = category_data[year_key]["categories"]
    year_categories = {key for key, value in interop_data[year_key]["focus_areas"].items() if value["countsTowardScore"]}
    return [item for item in all_categories if item["name"] in year_categories]


def load_wptreport(path: str) -> Mapping[str, Any]:
    rv = {}
    opener = gzip.GzipFile if is_gzip(path) else open
    with opener(path) as f:  # type: ignore
        try:
            data = json.load(f)
        except Exception as e:
            raise IOError(f"Failed to read {path}") from e
    for item in data["results"]:
        result = {"status": item["status"], "subtests": []}
        for subtest in item["subtests"]:
            result["subtests"].append(
                {"id": subtest["name"], "status": subtest["status"]}
            )
        rv[item["test"]] = result
    return rv


def load_taskcluster_results(log_paths: Iterable[str],
                             all_tests: Set[str]) -> Mapping[str, Any]:
    run_results = {}
    for path in log_paths:
        log_results = load_wptreport(path)
        for test_name, results in log_results.items():
            if test_name not in all_tests:
                continue
            if results["status"] == "SKIP":
                # Sometimes we have multiple jobs which log SKIP for tests that aren't run
                continue
            if test_name in run_results:
                print(f"  Warning: got duplicate results for {test_name}")
            run_results[test_name] = results
    return run_results


def score_wptreports(
    run_logs: Iterable[Iterable[str]],
    year: int = 2023,
    category_filter: Optional[Callable[[str], bool]] = None
) -> Mapping[str, List[int]]:
    """Get Interop scores from a list of paths to wptreport files

    :param runs: A list/iterable with one item per run. Each item is a
    list of wptreport files for that run.
    :param year: Integer year for which to calculate interop scores.
    :param:

    """
    category_data = fetch_category_data()
    interop_data = fetch_interop_data()
    labelled_tests = fetch_labelled_tests()

    categories = categories_for_year(year, category_data, interop_data)

    tests_by_category = {}
    all_tests = set()
    for category in categories:
        if category_filter is not None and not category_filter(category["name"]):
            continue
        tests = set()
        for label in category["labels"]:
            tests |= labelled_tests.get(label, set())
        tests_by_category[category["name"]] = tests
        all_tests |= tests

    runs_results = []
    for log_paths in run_logs:
        runs_results.append(load_taskcluster_results(log_paths, all_tests))

    run_scores, _ = _wpt_interop.interop_score(runs_results, tests_by_category, set())

    return run_scores
