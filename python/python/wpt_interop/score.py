import gzip
import json
import shutil
import tempfile
from collections import defaultdict
from typing import Any, Mapping

import fetchlogs
import requests

from . import _wpt_interop

CATEGORY_URL = "https://raw.githubusercontent.com/web-platform-tests/results-analysis/main/interop-scoring/category-data.json"
METADATA_URL = "https://wpt.fyi/api/metadata?includeTestLevel=true&product=chrome"

def fetch_category_data() -> Mapping[str, Mapping[str, any]]:
    return requests.get(CATEGORY_URL).json()


def fetch_labelled_tests():
    rv = defaultdict(set)
    data = requests.get(METADATA_URL).json()
    for test, metadata in data.items():
        for meta_item in metadata:
            if "label" in meta_item:
                rv[meta_item["label"]].add(test)
    return rv


def load_wptreport(path: str) -> Mapping[str, Any]:
    print(f"load_wptreport {path}")
    rv = {}
    opener = gzip.GzipFile if path.endswith(".gz") else open
    with opener(path) as f:
        data = json.load(f)
    for item in data["results"]:
        result = {"status": item["status"],
                  "subtests": []}
        for subtest in item["subtests"]:
            result["subtests"].append({"id": subtest["name"],
                                       "status": subtest["status"]})
        rv[item["test"]] = result
    return rv


def read_logs(branch, commit, task_filters, log_dir=None):
    cleanup_log_dir = False
    if log_dir is None:
        cleanup_log_dir = True
        log_dir = tempfile.mkdtemp()

    try:
        paths = fetchlogs.download_artifacts(branch,
                                             commit,
                                             task_filters=task_filters,
                                             out_dir=log_dir)
        for path in paths:
            yield load_wptreport(path)
    finally:
        if cleanup_log_dir:
            shutil.rmtree(log_dir)


def load_taskcluster_results(branch, commit, task_filters, log_dir=None) -> Mapping[str, Any]:
    run_results = {}
    for log_results in read_logs(branch, commit, task_filters, log_dir):
        for test_name, results in log_results.items():
            if test_name in run_results:
                print(f"Warning: got duplicate results for {test_name}")
            run_results[test_name] = results
    return run_results


def score_taskcluster_runs(runs, task_filters, year=2023, log_dir=None):
    categories = fetch_category_data()[str(year)]["categories"]
    labelled_tests = fetch_labelled_tests()

    tests_by_category = {}
    for category in categories:
        tests = set()
        for label in category["labels"]:
            tests |= labelled_tests.get(label, set())
        tests_by_category[category["name"]] = tests

    run_results = []
    for branch, commit in runs:
        run_results.append(load_taskcluster_results(branch, commit, task_filters, log_dir))

    run_scores, _ = _wpt_interop.interop_score(run_results, tests_by_category, set())

    return run_scores
