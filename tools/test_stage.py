#!/usr/bin/env python3
"""Audit the actual SDK-compiled patch and its pinned, untouched source export."""
import json
import subprocess
import sys
import horizon


def main():
    args = horizon.parser().parse_args(['check'] + sys.argv[1:])
    original = horizon.checkout_snapshot(args.horizon)
    if horizon.REPORT.exists():
        record = json.loads(horizon.REPORT.read_text())
        current = horizon.package.package()['sha256']
        if record['artifact']['sha256'] != current:
            record = horizon.build(args)
    else:
        record = horizon.build(args)
    report = horizon.audit(record)
    if original != horizon.checkout_snapshot(args.horizon):
        raise ValueError('original checkout changed during the SDK audit')
    report['original_checkout_unchanged'] = True
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    try:
        main()
    except (ValueError, subprocess.CalledProcessError) as error:
        raise SystemExit(str(error)) from error
