#!/usr/bin/env python3
"""Check fresh audit reports against an explicit, expiring warning review."""
import argparse
from datetime import date, datetime, timezone
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def review(cargo, npm, policy, today):
    if cargo.get('vulnerabilities', {}).get('count') != 0:
        raise ValueError('Cargo vulnerability report is missing or has vulnerabilities')
    if npm.get('metadata', {}).get('vulnerabilities', {}).get('total') != 0:
        raise ValueError('npm vulnerability report is missing or has vulnerabilities')
    database = cargo['database']
    updated = datetime.fromisoformat(database['last-updated']).date()
    if not 0 <= (today - updated).days <= 30:
        raise ValueError('advisory database must be at most 30 days old')
    if today > date.fromisoformat(policy['review_expires']):
        raise ValueError('dependency warning review has expired')
    expected = {(r['kind'], r['advisory'], r['package'], r['version'])
                for r in policy['warnings']}
    actual = set()
    for kind, entries in cargo.get('warnings', {}).items():
        for entry in entries:
            actual.add((kind, entry.get('advisory', {}).get('id'),
                        entry['package']['name'], entry['package']['version']))
    if actual != expected:
        raise ValueError('dependency warning inventory changed; review additions and removals')
    return {
        'schema': 1,
        'reviewed_at': today.isoformat(),
        'database_revision': database['last-commit'],
        'cargo_vulnerabilities': 0,
        'npm_vulnerabilities': 0,
        'reviewed_warnings': sorted([list(r) for r in actual]),
        'release_blockers': policy['release_blockers'],
        'release_ready': not policy['release_blockers'],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cargo', type=Path, required=True)
    parser.add_argument('--npm', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--require-release-ready', action='store_true')
    args = parser.parse_args()
    policy = json.loads((ROOT / 'distribution/dependency-review.json').read_text(encoding='utf-8'))
    result = review(json.loads(args.cargo.read_text(encoding='utf-8')), json.loads(args.npm.read_text(encoding='utf-8')),
                    policy, datetime.now(timezone.utc).date())
    result['lockfiles'] = {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest()
                           for name in ('Cargo.lock', 'clients/web/package-lock.json')}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    if args.require_release_ready and not result['release_ready']:
        raise SystemExit('Reviewed dependency blockers remain; see distribution/dependency-review.json')
    print(f"Reviewed dependency reports; release_ready={result['release_ready']}")


if __name__ == '__main__':
    main()
