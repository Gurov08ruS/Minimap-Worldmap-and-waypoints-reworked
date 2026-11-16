#!/usr/bin/env python3
"""Summarize MWWR_PROFILE=1 CPU timing lines from a Horizon client log.

Percentiles are maxima of reported windows, never averages or a fabricated
whole-run percentile. GPU execution and total game FPS are not measured here.
"""
import argparse
import json
from pathlib import Path
import re
import sys


LINE = re.compile(
    r'\[mwwr-profile\] stage=(\S+) count=(\d+) p95_us=(\d+) '
    r'p99_us=(\d+) max_us=(\d+) items=(\d+) bytes=(\d+)'
)


def summarize(lines):
    stages = {}
    for line in lines:
        match = LINE.search(line)
        if not match:
            continue
        name, count, p95, p99, maximum, items, size = match.groups()
        row = stages.setdefault(name, dict(windows=0, count=0, worst_p95_us=0,
                                          worst_p99_us=0, max_us=0, items=0, bytes=0))
        row['windows'] += 1
        row['count'] += int(count)
        row['worst_p95_us'] = max(row['worst_p95_us'], int(p95))
        row['worst_p99_us'] = max(row['worst_p99_us'], int(p99))
        row['max_us'] = max(row['max_us'], int(maximum))
        row['items'] += int(items)
        row['bytes'] += int(size)
    return dict(sorted(stages.items()))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('log', type=Path, nargs='?', help='client log; stdin if omitted')
    parser.add_argument('--json', action='store_true')
    args = parser.parse_args()
    if args.log:
        with args.log.open(encoding='utf-8', errors='replace') as source:
            stages = summarize(source)
    else:
        stages = summarize(sys.stdin)
    if not stages:
        parser.error('no profiling windows found; set MWWR_PROFILE=1 for the client process')
    if args.json:
        print(json.dumps(stages, indent=2))
    else:
        print('stage                     calls  worst p95 µs  worst p99 µs      bytes      items')
        for name, row in stages.items():
            print(f"{name:25} {row['count']:6} {row['worst_p95_us']:13} "
                  f"{row['worst_p99_us']:13} {row['bytes']:10} {row['items']:10}")


if __name__ == '__main__':
    main()
