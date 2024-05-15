import json
import logging
import os
from datetime import datetime, timedelta
from types import TracebackType
from typing import Any, ContextManager, Iterable, Iterator, Mapping, MutableMapping, Optional, Union
from urllib.parse import urlencode

import requests

RUNS_URL = 'https://wpt.fyi/api/runs'

RunsByDate = Mapping[str, list["RevisionRuns"]]

logger = logging.getLogger("wpt_interop.runs")


class Run:
    def __init__(self, run_id: int,
                 browser_name: str,
                 browser_version: str,
                 os_name: str,
                 os_version: str,
                 revision: str,
                 full_revision_hash: str,
                 results_url: str,
                 created_at: datetime,
                 time_start: datetime,
                 time_end: datetime,
                 raw_results_url: str,
                 labels: list[str]):
        self.run_id = run_id
        self.browser_name = browser_name
        self.browser_version = browser_version
        self.os_name = os_name
        self.os_version = os_version
        self.revision = revision
        self.full_revision_hash = full_revision_hash
        self.results_url = results_url
        self.created_at = created_at
        self.time_start = time_start
        self.time_end = time_end
        self.raw_results_url = raw_results_url
        self.labels = labels

    @classmethod
    def from_json(cls, data: Mapping[str, Any]) -> "Run":
        return cls(data["id"],
                   data["browser_name"],
                   data["browser_version"],
                   data["os_name"],
                   data["os_version"],
                   data["revision"],
                   data["full_revision_hash"],
                   data["results_url"],
                   datetime.fromisoformat(data["created_at"]),
                   datetime.fromisoformat(data["time_start"]),
                   datetime.fromisoformat(data["time_end"]),
                   data["raw_results_url"],
                   data["labels"])

    def to_json(self) -> Mapping[str, Union[str, int, list[str]]]:
        return {
            "id": self.run_id,
            "browser_name": self.browser_name,
            "browser_version": self.browser_version,
            "os_name": self.os_name,
            "os_version": self.os_version,
            "revision": self.revision,
            "full_revision_hash": self.full_revision_hash,
            "results_url": self.results_url,
            "created_at": self.created_at.isoformat(),
            "time_start": self.time_start.isoformat(),
            "time_end": self.time_end.isoformat(),
            "raw_results_url": self.raw_results_url,
            "labels": self.labels
        }


class RevisionRuns:
    # Runs for a specific revision
    def __init__(self, revision: str, runs: list[Run]):
        self.revision = revision
        self.runs = runs

    def __len__(self) -> int:
        return len(self.runs)

    def __iter__(self) -> Iterator[Run]:
        yield from self.runs

    def append(self, run: Run) -> None:
        self.runs.append(run)

    def extend(self, other: Iterable[Run]) -> None:
        self.runs.extend(other)

    @property
    def min_start_time(self) -> datetime:
        return min(item.time_start for item in self.runs)

    def run_ids(self) -> list[int]:
        return [item.run_id for item in self.runs]

    def is_aligned(self, products: list[str]) -> bool:
        # Check if we have a run for each product
        return {item.browser_name for item in self.runs} == set(products)


class RunsByRevision:
    def __init__(self, runs: list[RevisionRuns]) -> None:
        self._runs = runs
        self._make_index()

    def _make_index(self) -> None:
        self._runs.sort(key=lambda x: x.min_start_time)
        self._index = {}
        for run in self._runs:
            self._index[run.revision] = run

    def __iter__(self) -> Iterator[RevisionRuns]:
        """Iterator over runs in date order"""
        for run in self._runs:
            yield run

    def __contains__(self, revision: str) -> bool:
        return revision in self._index

    def __getitem__(self, revision: str) -> RevisionRuns:
        return self._index[revision]

    def filter_by_revisions(self, revisions: set[str]) -> "RunsByRevision":
        return RunsByRevision([item for item in self._runs if item.revision in revisions])


def group_by_date(runs_by_revision: RunsByRevision) -> RunsByDate:
    runs_by_date: dict[str, list[RevisionRuns]] = {}
    for rev_runs in runs_by_revision:
        date = rev_runs.min_start_time.strftime("%Y-%m-%d")
        if date not in runs_by_date:
            runs_by_date[date] = []
        runs_by_date[date].append(rev_runs)

    return runs_by_date


def fetch_runs(products: list[str],
               channel: str,
               from_date: Optional[datetime] = None,
               to_date: Optional[datetime] = None,
               aligned: bool = True,
               max_per_day: Optional[int] = None,
               run_cache: Optional[ContextManager["RunCacheData"]] = None
               ) -> RunsByRevision:

    """Fetch all the runs for a given date range.

    Runs are only fetched if they aren't found (keyed by date) in the run_cache."""

    revision_index: MutableMapping[str, int] = {}
    rv: list[RevisionRuns] = []

    now = datetime.now()
    if from_date is None:
        from_date = datetime(now.year, 1, 1)
    if to_date is None:
        to_date = datetime(now.year, now.month, now.day)

    query = [
        ("label", "master"),
        ("label", channel),
    ]
    for product in products:
        query.append(("product", product))
    if aligned:
        query.append(("aligned", "true"))
    if max_per_day:
        query.append(("max-count", str(max_per_day)))

    url = f"{RUNS_URL}?{urlencode(query)}"

    fetch_date = from_date
    cache_cutoff_date = now - timedelta(days=3)

    if run_cache is None:
        run_cache = RunCache(products, channel, aligned, max_per_day)
    assert run_cache is not None

    with run_cache as cache:
        while fetch_date < to_date:
            next_date = fetch_date + timedelta(days=1)

            if fetch_date in cache and fetch_date < cache_cutoff_date:
                logger.debug(f"Using cached data for {fetch_date.strftime('%Y-%m-%d')}")
                day_runs = cache[fetch_date]
            else:
                date_query = urlencode({
                    "from": fetch_date.strftime("%Y-%m-%d"),
                    "to": next_date.strftime("%Y-%m-%d")
                })
                date_url = f"{url}&{date_query}"
                logger.info(f"Fetching runs from {date_url}")
                day_runs = requests.get(date_url).json()
                cache[fetch_date] = day_runs

            by_revision = group_by_revision(day_runs)
            for revision, runs in by_revision.items():
                if revision not in revision_index:
                    idx = len(rv)
                    revision_index[revision] = idx
                    rv.append(RevisionRuns(revision, []))

                rv[revision_index[revision]].extend(runs)

            fetch_date = next_date

    return RunsByRevision(rv)


def group_by_revision(runs: list[Mapping[str, Any]]) -> Mapping[str, list[Run]]:
    rv: dict[str, list[Run]] = {}
    for run_json in runs:
        run = Run.from_json(run_json)
        if run.full_revision_hash not in rv:
            rv[run.full_revision_hash] = []
        rv[run.full_revision_hash].append(run)
    return rv


class RunCacheData:
    """Run cache that stores a map of {date: [Run as JSON]}, matching the fetch_runs endpoint"""
    def __init__(self, data: MutableMapping[str, Any]):
        self.data = data

    def __contains__(self, date: datetime) -> bool:
        return date.strftime("%Y-%m-%d") in self.data

    def __getitem__(self, date: datetime) -> list[Mapping[str, Any]]:
        return self.data[date.strftime("%Y-%m-%d")]

    def __setitem__(self, date: datetime, value: list[Mapping[str, Any]]) -> None:
        self.data[date.strftime("%Y-%m-%d")] = value


class RunCache:
    def __init__(self,
                 products: list[str],
                 channel: str,
                 aligned: bool = True,
                 max_per_day: Optional[int] = None):
        products_str = "-".join(products)

        self.path = (f"products:{products_str}-channel:{channel}-"
                     f"aligned:{aligned}-max_per_day:{max_per_day}.json")
        self.data: Optional[RunCacheData] = None

    def __enter__(self) -> RunCacheData:
        if os.path.exists(self.path):
            with open(self.path) as f:
                try:
                    data = json.load(f)
                except json.JSONDecodeError:
                    data = {}
        if self.data is None:
            data = {}
        self.data = RunCacheData(data)
        return self.data

    def __exit__(self,
                 exc_type: Optional[type[BaseException]],
                 exc_val: Optional[BaseException],
                 exc_tb: Optional[TracebackType]) -> None:
        if self.data is not None:
            with open(self.path, "w") as f:
                json.dump(self.data.data, f)
            self.data = None
