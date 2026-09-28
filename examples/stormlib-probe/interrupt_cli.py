"""Kill the unpack CLI on a progress event, then verify standalone recovery."""
import argparse
import hashlib
import json
import shutil
import subprocess
import threading
from pathlib import Path

if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('executable', type=Path)
    p.add_argument('archive', type=Path)
    p.add_argument('new_work_directory', type=Path)
    p.add_argument('--phase', choices=['extract', 'backup'], default='extract')
    args = p.parse_args()
    args.new_work_directory.mkdir(parents=True, exist_ok=False)
    root = args.new_work_directory / args.archive.stem
    root.mkdir()
    source = root / args.archive.name
    shutil.copyfile(args.archive, source)
    expected = hashlib.sha256(source.read_bytes()).hexdigest()
    process = subprocess.Popen([str(args.executable.resolve()), 'unpack-mpq', '--source', str(source.resolve()), '--events'],
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding='utf-8')
    timer = threading.Timer(120, process.kill)
    timer.start()
    interrupted = False
    try:
        for line in process.stdout:
            event = json.loads(line)
            if event.get('phase') == args.phase:
                process.kill()
                interrupted = True
                break
        process.communicate(timeout=30)
    finally:
        timer.cancel()
        if process.poll() is None:
            process.kill()
            process.wait()
    assert interrupted, 'did not reach the requested interruption boundary'
    recovered = subprocess.run([str(args.executable.resolve()), 'recover-mpq', '--mod-directory', str(root.resolve()), '--json'],
                               capture_output=True, text=True, encoding='utf-8', timeout=120, check=True)
    report = json.loads(recovered.stdout)
    assert not (root / '.d2rhub-unpack.json').exists()
    if source.is_file():
        assert hashlib.sha256(source.read_bytes()).hexdigest() == expected
        assert report['status'] == 'rolled_back'
    else:
        assert args.phase == 'backup' and report['status'] == 'committed'
        assert hashlib.sha256(Path(report['backup_path']).read_bytes()).hexdigest() == expected
    repeated = subprocess.run([str(args.executable.resolve()), 'recover-mpq', '--mod-directory', str(root.resolve()), '--json'],
                              capture_output=True, text=True, encoding='utf-8', timeout=120, check=True)
    assert json.loads(repeated.stdout)['status'] == 'no_transaction'
    print(json.dumps({'phase': args.phase, 'recovery': report['status'], 'archive_sha256': expected, 'passed': True}))
