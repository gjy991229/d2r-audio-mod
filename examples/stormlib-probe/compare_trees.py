"""Read-only SHA-256 comparison of an extracted tree and a GUI reference."""
import argparse
import hashlib
import json
from pathlib import Path


def digest(path):
    result = hashlib.sha256()
    with path.open('rb') as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b''):
            result.update(block)
    return result.hexdigest()


def inventory(root):
    return {
        p.relative_to(root).as_posix(): {'size': p.stat().st_size, 'sha256': digest(p)}
        for p in root.rglob('*') if p.is_file()
    }


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('reference', type=Path)
    parser.add_argument('extracted', type=Path)
    parser.add_argument('--archive', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    if not args.reference.is_dir() or not args.extracted.is_dir():
        parser.error('both input trees must exist')
    left, right = inventory(args.reference), inventory(args.extracted)
    missing = sorted(left.keys() - right.keys())
    extra = sorted(right.keys() - left.keys())
    changed = sorted(k for k in left.keys() & right.keys() if left[k] != right[k])
    report = {
        'archive': str(args.archive.resolve()), 'archive_sha256': digest(args.archive),
        'archive_bytes': args.archive.stat().st_size,
        'reference': str(args.reference.resolve()), 'extracted': str(args.extracted.resolve()),
        'reference_count': len(left), 'extracted_count': len(right),
        'total_bytes': sum(x['size'] for x in left.values()),
        'zero_byte_files': sum(x['size'] == 0 for x in left.values()),
        'missing': missing, 'extra': extra, 'changed': changed,
        'identical': not (missing or extra or changed),
        'files': right,
    }
    args.report.parent.mkdir(parents=True, exist_ok=True)
    with args.report.open('x', encoding='utf-8') as handle:
        json.dump(report, handle, ensure_ascii=False, indent=2)
    print(json.dumps({k: v for k, v in report.items() if k != 'files'}, ensure_ascii=True, indent=2))
    raise SystemExit(0 if report['identical'] else 1)
