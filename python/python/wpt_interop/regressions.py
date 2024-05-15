import csv
import sys
from datetime import datetime, timedelta

from . import _wpt_interop
from .repo import ResultsAnalysisCache, Metadata
from .runs import fetch_runs

def main() -> None:
    results_analysis_repo = ResultsAnalysisCache(None, None)
    metadata_repo = Metadata(None, None)

    for repo in [results_analysis_repo, metadata_repo]:
        repo.update()


    now = datetime.now()
    from_date = datetime(now.year, now.month, now.day) - timedelta(days=7)
    browser_names = ["firefox", "firefox_android"]
    runs = fetch_runs(browser_names, "experimental", from_date=from_date, aligned=True)
    if not runs:
        print("No aligned runs found in the last 7 days")
        return

    latest_rev_runs = list(runs)[-1]
    by_browser_name = {item.browser_name: item for item in latest_rev_runs}

    regressions = _wpt_interop.regressions(results_analysis_repo.path,
                                           metadata_repo.path,
                                           (by_browser_name[browser_names[0]].run_id,
                                            by_browser_name[browser_names[1]].run_id))

    result_header = f"{browser_names[1]} Result"
    writer = csv.DictWriter(sys.stdout, ["Test", "Subtest", result_header, "Labels"])
    writer.writeheader()
    for test, results in sorted(regressions.items()):
        test_result, subtest_results, labels = results
        writer.writerow({"Test": test,
                         "Subtest": "",
                         result_header: test_result if test_result is not None else "",
                         "Labels": ",".join(labels)})
        for subtest, new_result in sorted(subtest_results):
            writer.writerow({"Test": "",
                             "Subtest": subtest,
                             result_header: new_result})


if __name__ == "__main__":
    main()
