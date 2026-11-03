#!/usr/bin/env python3
"""Keep small diagnostics locally; archive closed captures with byte verification.

Copy verification is not validation of the Apple capture contents. No source is
deleted, and an existing destination is never replaced by the archive operation.
"""
import argparse
import hashlib
import json
import plistlib
import shutil
import subprocess
import tarfile
from pathlib import Path

SMALL = ('provenance.json', 'replay.json', 'summary.json', 'hardware-summary.json',
         'failure.json', 'window-*/timeline.json', 'window-*/counters.json',
         'window-*/window-*.json', 'window-*/exports/availability.json')


def digest(path):
    with path.open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()


def capture_filesystem(path):
    """Use a local APFS working directory; external captures failed on this host."""
    parent = path.resolve()
    while not parent.exists():
        parent = parent.parent
    info = plistlib.loads(subprocess.check_output(['diskutil', 'info', '-plist', str(parent)]))
    fs = info.get('FilesystemType', '')
    if fs.lower() != 'apfs':
        raise ValueError('Capture output must be staged on APFS; archive it after profiling. '
                         f'{parent} uses {fs or "unknown filesystem"}')
    return fs


def default_mirror(root):
    suffix = hashlib.sha256(str(root.resolve()).encode()).hexdigest()[:12]
    return Path(__file__).resolve().parents[3] / 'out/profile-summaries' / (root.name+'-'+suffix)


def mirror_summaries(root, destination=None):
    destination = destination or default_mirror(root)
    destination.mkdir(parents=True, exist_ok=True)
    entries = []
    for p in sorted({p for pattern in SMALL for p in root.glob(pattern)}):
        if not p.is_file() or p.is_symlink():
            continue
        if p.stat().st_size > 8*1024*1024:
            raise ValueError('Expected small summary, refusing large file: '+str(p))
        rel = p.relative_to(root)
        target = destination / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        temporary = target.with_suffix(target.suffix+'.partial')
        shutil.copyfile(p, temporary)
        h = digest(p)
        if digest(temporary) != h:
            raise ValueError('Summary changed during copy: '+str(p))
        temporary.replace(target)
        entries.append(dict(path=str(rel), sha256=h, bytes=p.stat().st_size))
    (destination/'mirror.json').write_text(json.dumps(dict(source=str(root.resolve()), files=entries), indent=2)+'\n')
    return destination


def archive_capture(source, destination):
    if source.suffix != '.gputrace' or not source.is_dir():
        raise ValueError('Expected a closed .gputrace directory')
    if destination.exists():
        raise ValueError('Archive destination already exists')
    if destination.suffix != '.tar':
        raise ValueError('Use a .tar archive to preserve capture symlinks on exFAT')
    paths = sorted(source.rglob('*'));manifest = {}
    for p in paths:
        rel=str(p.relative_to(source))
        if p.is_symlink():
            link=p.readlink()
            if link.is_absolute() or not p.resolve(strict=True).is_relative_to(source.resolve()):
                raise ValueError('Capture link escapes source: '+rel)
            manifest[rel]=dict(symlink=str(link))
        elif p.is_file():
            manifest[rel]=dict(bytes=p.stat().st_size,sha256=digest(p))
    if not manifest:raise ValueError('Empty capture')
    with tarfile.open(destination,'w',dereference=False) as archive:
        archive.add(source,arcname=source.name)
    with tarfile.open(destination,'r') as archive:
        for rel,data in manifest.items():
            entry=archive.getmember(source.name+'/'+rel)
            if 'symlink' in data:
                if not entry.issym() or entry.linkname!=data['symlink']:raise ValueError('Archive link mismatch: '+rel)
            else:
                with archive.extractfile(entry) as content:
                    if hashlib.file_digest(content,'sha256').hexdigest()!=data['sha256']:
                        raise ValueError('Archive byte verification failed: '+rel)
    report = dict(source=str(source.resolve()), destination=str(destination.resolve()),
                  byte_verified=True, apple_semantic_validation=False, symlinks_preserved=True,files=manifest)
    destination.with_suffix('.archive.json').write_text(json.dumps(report, indent=2)+'\n')
    return report


def main():
    p = argparse.ArgumentParser(description=__doc__)
    sub = p.add_subparsers(dest='mode', required=True)
    m = sub.add_parser('mirror'); m.add_argument('--run', type=Path, required=True); m.add_argument('--output', type=Path)
    a = sub.add_parser('archive'); a.add_argument('--trace', type=Path, required=True); a.add_argument('--output', type=Path, required=True)
    args = p.parse_args()
    if args.mode == 'mirror':
        print(mirror_summaries(args.run, args.output))
    else:
        result = archive_capture(args.trace, args.output)
        print('byte-verified', len(result['files']), 'files; source retained')


if __name__ == '__main__':
    main()
