import csv
import gzip
import json
import os
import subprocess
from collections import defaultdict
from datetime import datetime, timedelta
from functools import cache
from typing import Any, Callable, Iterable, List, Mapping, Optional, Set, Tuple
from urllib.parse import urlencode

import requests

from . import _wpt_interop  # type: ignore

CATEGORY_URL = "https://raw.githubusercontent.com/web-platform-tests/results-analysis/main/interop-scoring/category-data.json"
INTEROP_DATA_URL = "https://wpt.fyi/static/interop-data.json"
METADATA_URL = "https://wpt.fyi/api/metadata?includeTestLevel=true&product=chrome"
RUNS_URL = 'https://wpt.fyi/api/runs';

DEFAULT_RESULTS_CACHE_PATH = os.path.join(os.path.abspath(os.curdir),
                                          "results-analysis-cache.git")


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


class RunCache:
    def __init__(self,
                 products: List[str],
                 experimental: bool,
                 aligned: bool = True,
                 max_per_day: Optional[int] = None):
        products_str = "-".join(products)
        branch = "experimental" if experimental else "stable"

        self.path = f"products:{products_str}-branch:{branch}-aligned:{aligned}-max_per_day:{max_per_day}.json"
        self.data = None

    def __enter__(self):
        if os.path.exists(self.path):
            with open(self.path) as f:
                self.data = json.load(f)
        if self.data is None:
            self.data = {}
        return self

    def __exit__(self, *args, **kwargs):
        with open(self.path, "w") as f:
            json.dump(self.data, f)

    def __contains__(self, date):
        return date.strftime("%Y-%m-%d") in self.data

    def __getitem__(self, date):
        return self.data[date.strftime("%Y-%m-%d")]

    def __setitem__(self, date, value):
        self.data[date.strftime("%Y-%m-%d")] = value


def fetch_runs(products: List[str],
               experimental: bool,
               from_date: Optional[datetime] = None,
               to_date: Optional[datetime] = None,
               aligned: bool = True,
               max_per_day: Optional[int] = None
               ) -> Mapping[datetime, Mapping[str, List[Mapping[str, Any]]]]:

    runs = {}
    now = datetime.now()
    if from_date is None:
        from_date = datetime(now.year, 1, 1)
    if to_date is None:
        to_date = datetime(now.year, now.month, now.day)

    query = [
        ("label", "master"),
        ("label", "experimental" if experimental else "stable"),
    ]
    for product in products:
        query.append(("product", product))
    if aligned:
        query.append(("aligned", "true"))
    if max_per_day:
        query.append(("max-count", str(max_per_day)))

    url = f"{RUNS_URL}?{urlencode(query)}"

    fetch_date = from_date

    with RunCache(products, experimental, aligned, max_per_day) as run_cache:

        while fetch_date < to_date:
            next_date = fetch_date + timedelta(days=1)

            if fetch_date in run_cache:
                # TODO: Don't use the cache for recent dates
                data = run_cache[fetch_date]
            else:
                date_query = urlencode({
                    "from": fetch_date.strftime("%Y-%m-%d"),
                    "to": next_date.strftime("%Y-%m-%d")
                })
                date_url = f"{url}&{date_query}"
                print(date_url)
                data = requests.get(date_url).json()

            if data:
                if aligned and len(data) != len(products):
                    raise ValueError(f"Got {len(data)} runs, expected {len(products)}")
                run_cache[fetch_date] = data
                if not fetch_date in runs:
                    runs[fetch_date] = {}
                for run in data:
                    if not run["revision"] in runs[fetch_date]:
                        runs[fetch_date][run["revision"]] = []
                    runs[fetch_date][run["revision"]].append(run)
            else:
                run_cache[fetch_date] = []

            fetch_date = next_date

    return runs


def is_gzip(path: str) -> bool:
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
                        interop_data: Mapping[str, Mapping[str, Any]],
                        only_active: bool = True) -> List[Mapping[str, Any]]:
    year_key = str(year)
    if year_key not in category_data or year_key not in interop_data:
        raise ValueError(f"Invalid year {year}")
    all_categories = category_data[year_key]["categories"]
    year_categories = {key for key, value in interop_data[year_key]["focus_areas"].items()
                       if (not only_active or value["countsTowardScore"])}
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
                {"name": subtest["name"], "status": subtest["status"]}
            )
        rv[item["test"]] = result
    return rv


def load_taskcluster_results(log_paths: Iterable[str],
                             all_tests: Set[str],
                             expected_failures: Mapping[str, Set[Optional[str]]]) -> Mapping[str, Any]:
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
            if test_name in expected_failures:
                if None in expected_failures[test_name]:
                    run_results[test_name]["expected"] = "FAIL"
                else:
                    for subtest_result in run_results[test_name]:
                        if subtest_result["name"] in expected_failures[test_name]:
                            subtest_result["expected"] = "FAIL"
    return run_results


@cache
def get_category_data(year: int,
                      only_active=True,
                      category_filter: Optional[Callable[[str], bool]] = None
                      ) -> Tuple[Mapping[str, Set[str]], Set[str]]:
    category_data = fetch_category_data()
    interop_data = fetch_interop_data()
    labelled_tests = fetch_labelled_tests()

    categories = categories_for_year(year, category_data, interop_data, only_active)

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

    return tests_by_category, all_tests


def date_range(year: int) -> Tuple[datetime, datetime]:
    now = datetime.now()
    from_date = datetime(year, 1, 1)
    if now.year == year:
        to_date = datetime(year, now.month, now.day)
    else:
        to_date = datetime(year, 12, 31)
    return from_date, to_date


def update_results_cache(path: str):
    if not os.path.exists(path):
        os.makedirs(path)
        subprocess.run(["git", "init", "--bare"], cwd=path)
    subprocess.run(["git", "fetch", "--tags", "https://github.com/web-platform-tests/results-analysis-cache"], cwd=path)


def score_runs_by_date(runs_by_date: Mapping[datetime, Mapping[str, Mapping[str, Any]]],
                       tests_by_category: Mapping[str, Set[str]],
                       results_cache_path: str = DEFAULT_RESULTS_CACHE_PATH):
    results_by_date = {}

    for date, runs_by_revision in runs_by_date.items():
        results_by_date[date] = {}
        for revision, runs in runs_by_revision.items():
            print(f"Scoring {date.strftime('%Y-%m-%d')}: {revision}")
            results_by_date[date][revision] = {}

            run_ids = [item["id"] for item in runs]

            browser_scores, interop_scores, _ = _wpt_interop.score_runs(results_cache_path, run_ids, tests_by_category, set())
            for i, run in enumerate(runs):
                run_score = {}
                for category in browser_scores.keys():
                    run_score[category] = browser_scores[category][i]
                run_score["version"] = run["browser_version"]

                results_by_date[date][revision][run["browser_name"]] = run_score
            results_by_date[date][revision]["interop"] = interop_scores

    return results_by_date


def score_wptreports(
    run_logs: Iterable[Iterable[str]],
    year: int = 2023,
    category_filter: Optional[Callable[[str], bool]] = None,
    expected_failures: Optional[Mapping[str, Set[Optional[str]]]] = None,
) -> Tuple[Mapping[str, List[int]], Optional[Mapping[str, List[int]]]]:
    """Get Interop scores from a list of paths to wptreport files

    :param runs: A list/iterable with one item per run. Each item is a
    list of wptreport files for that run.
    :param year: Integer year for which to calculate interop scores.
    :param:

    """
    include_expected_failures = expected_failures is not None
    if not include_expected_failures:
        expected_failures = {}

    tests_by_category, all_tests = get_category_data(year, category_filter=category_filter)
    runs_results = []
    for log_paths in run_logs:
        runs_results.append(load_taskcluster_results(log_paths, all_tests, expected_failures))

    run_scores, _, expected_failure_scores = _wpt_interop.interop_score(runs_results, tests_by_category, set())

    if not include_expected_failures:
        # Otherwise this will just be all zeros
        expected_failure_scores = None

    return run_scores, expected_failure_scores


def score_runs(year: int,
               run_ids: Iterable[int],
               results_cache_path: str = DEFAULT_RESULTS_CACHE_PATH,
               category_filter: Optional[Callable[[str], bool]] = None):
    tests_by_category, all_tests = get_category_data(year, category_filter=category_filter)

    update_results_cache(results_cache_path)

    return _wpt_interop.score_runs(results_cache_path, run_ids, tests_by_category, set())


def score_aligned_runs(year: int,
                       only_active: bool = True,
                       results_cache_path: str = DEFAULT_RESULTS_CACHE_PATH,
                       products: Optional[Iterable[str]] = None,
                       experimental: bool = True,
                       max_per_day: int = 1) -> Mapping[datetime, Mapping[str, Mapping[str, Any]]]:
    if products is None:
        products = ["chrome", "edge", "firefox", "safari"]

    tests_by_category, all_tests = get_category_data(year)

    update_results_cache(results_cache_path)

    from_date, to_date = date_range(year)
    runs_by_date = fetch_runs(products, experimental, from_date, to_date, aligned=True, max_per_day=max_per_day)

    return score_runs_by_date(runs_by_date,
                              tests_by_category,
                              results_cache_path)


def write_per_date_csv(year: int,
                       results_cache_path: str = DEFAULT_RESULTS_CACHE_PATH,
                       products: Optional[List[str]]=None):
    if products is None:
        products = ["chrome", "edge", "firefox", "safari"]

    product_keys = products + ["interop"]

    tests_by_category, _ = get_category_data(year, only_active=False)
    categories = list(tests_by_category.keys())

    update_results_cache(results_cache_path)

    from_date, to_date = date_range(year)

    for experimental, label in [(True, "experimental"),
                                (False, "stable")]:

        filename = f"interop-{year}-{label}-v2.csv"
        with open(filename, "w") as f:
            writer = csv.writer(f)

            headers = ["date"]
            for product in product_keys:
                if product != "interop":
                    headers.append(f"{product}-version")
                for category in categories:
                    headers.append(f"{product}-{category}")

            writer.writerow(headers)

            runs_by_date = fetch_runs(products,
                                      experimental,
                                      from_date,
                                      to_date,
                                      aligned=True,
                                      max_per_day=1)
            results_by_date = score_runs_by_date(runs_by_date,
                                                 tests_by_category,
                                                 results_cache_path)

            for date, runs_by_revision in sorted(results_by_date.items(), key=lambda item: item[0]):

                assert len(runs_by_revision) == 1
                product_results = runs_by_revision[list(runs_by_revision.keys())[0]]
                row_data = [date.strftime("%Y-%m-%d")]
                for product in product_keys:
                    results = product_results[product]
                    if product != "interop":
                        row_data.append(results["version"])
                    for category in categories:
                        row_data.append(results[category])
                writer.writerow(row_data)
