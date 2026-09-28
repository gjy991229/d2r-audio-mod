"""Verify the production CLI output against a GUI reference, including name mappings."""
import argparse
import hashlib
import json
from pathlib import Path


def sha(path):
    h = hashlib.sha256()
    with path.open('rb') as f:
        for block in iter(lambda: f.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


def gui_raw_name(name):
    # Reverse the specific MPQEditor legacy escaping, never used by the CLI.
    result = bytearray()
    i = 0
    while i < len(name):
        if name[i] == '%' and i + 2 < len(name):
            try:
                result.append(int(name[i+1:i+3], 16))
                i += 3
                continue
            except ValueError:
                pass
        if ord(name[i]) > 255:
            return None
        result.append(ord(name[i]))
        i += 1
    return bytes(result)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('events', type=Path)
    parser.add_argument('reference', type=Path)
    parser.add_argument('original', type=Path)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    completed = [json.loads(line)['report'] for line in args.events.read_text(encoding='utf-8-sig').splitlines()
                 if json.loads(line).get('type') == 'completed']
    assert len(completed) == 1
    report = completed[0]
    backup = Path(report['backup_path'])
    output = Path(report['mpq_directory'])
    assert output.is_dir() and output.name.lower() == output.parent.name.lower() + '.mpq'
    assert sha(backup) == sha(args.original) == report['archive_sha256']
    records = json.loads((backup.parent / 'files.json').read_text(encoding='utf-8'))
    ignored = {'(listfile)', '(attributes)', '(signature)'}
    reference = {p.relative_to(args.reference).as_posix(): p for p in args.reference.rglob('*')
                 if p.is_file() and p.relative_to(args.reference).as_posix() not in ignored}
    aliases = {}
    for name, path in reference.items():
        key = gui_raw_name(name)
        if key is not None:
            assert key not in aliases, 'ambiguous GUI filename mapping'
            aliases[key] = path
    matched = set()
    for r in records:
        raw = bytes.fromhex(r['archive_name_hex']).replace(b'\\', b'/')
        expected = reference.get(r['path']) or aliases.get(raw)
        assert expected is not None, r
        assert expected not in matched, 'duplicate reference match'
        matched.add(expected)
        actual = output / r['path']
        assert actual.stat().st_size == expected.stat().st_size == r['size'], r['path']
        assert sha(actual) == sha(expected) == r['sha256'], r['path']
    actual_names = {p.relative_to(output).as_posix() for p in output.rglob('*') if p.is_file()}
    assert actual_names == {r['path'] for r in records}
    assert len(matched) == len(reference) == report['file_count']
    summary = {
        'verified': True, 'file_count': len(records),
        'zero_byte_files': sum(r['size'] == 0 for r in records),
        'total_bytes': sum(r['size'] for r in records),
        'backup_and_original_sha256': sha(backup),
        'escaped_file_names': report['escaped_file_names'],
        'note': 'Internal MPQ metadata omitted; escaped names matched by original archive bytes.',
    }
    with args.report.open('x', encoding='utf-8') as f:
        json.dump(summary, f, ensure_ascii=False, indent=2)
    print(json.dumps(summary, ensure_ascii=True, indent=2))
