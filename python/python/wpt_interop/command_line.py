import argparse
import math
import re

from . import score

repos = ["autoland", "mozilla-central", "try", "mozilla-central", "mozilla-beta", "wpt"]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--log-dir", action="store",
                        help="Directory into which to download logs")
    parser.add_argument("--year", action="store", default=2023, type=int,
                        help="Interop year to score against")
    parser.add_argument("--pdb", action="store_true",
                        help="Drop into debugger on error")
    parser.add_argument("--task-filter", dest="task_filters", action="append",
                        help="Filter applied to task names. Multiple filters must all match")
    parser.add_argument("commits", nargs="+",
                        help="repo:commit e.g. mozilla-central:fae24810aef1 for the runs to include")
    return parser.parse_args()


def print_scores(runs, results_by_category):
    tab = "\t" # For f-string
    header = "\t".join(f"{repo}:{commit}" for repo, commit in runs)
    print(f"\t{header}")
    totals = [0] * len(runs)
    for category, category_results in results_by_category.items():
        for i, result in enumerate(category_results):
            totals[i] += result
        print(f"{category}\t{tab.join(str(item) for item in category_results)}")
    totals = [math.floor(float(item) / len(results_by_category)) for item in totals]
    print(f"Total\t{tab.join(str(item) for item in totals)}")


def main():
    args = parse_args()
    runs = []
    for item in args.commits:
        if ":" not in item:
            raise ValueError(f"Expected commits of the form repo:commit, got {item}")
        repo, commit = item.split(":", 1)
        if repo not in repos:
            raise ValueError(f"Unsupported repo {repo}")
        runs.append((repo, commit))

    try:
        scores = score.score_taskcluster_runs(runs, task_filters=args.task_filters, year=args.year, log_dir=args.log_dir)
        print_scores(runs, scores)
    except Exception:
        if args.pdb:
            import pdb
            pdb.post_mortem()
        raise
