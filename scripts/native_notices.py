"""Distro attribution for the selected AppDir, independent of the product engine."""
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
from urllib.parse import quote

MANIFEST = 'usr/share/rstorrent/native-notices/manifest.json'
NOTICE_ROOT = str(PurePosixPath(MANIFEST).parent)
FIRST_PARTY = {'usr/bin/rstorrent-desktop', 'usr/bin/rstorrent-native-host'}
HELPERS = {'usr/bin/xdg-mime', 'usr/bin/xdg-open'}
# Exact Tauri apprun-old mirror assets; source/runtime obligations remain open.
APPRUN = {
    'f30140a43a0a59e46db21bdefdf749b9e9f2c6946e92afabbacf98b8ae73fb4f': 'x86_64',
    '072f17c0895a85c490282fe5395c5007e5fc75da727e553b3b8fb680feb11578': 'aarch64',
}
MAX_FILE = 16 * 1024**2
MAX_TOTAL = 64 * 1024**2


def sha(path):
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        while chunk := stream.read(65536):
            digest.update(chunk)
    return digest.hexdigest()


def safe_file(root, name):
    path = PurePosixPath(name)
    if path.is_absolute() or not path.parts or '..' in path.parts or '\\' in name:
        raise ValueError('unsafe native manifest path')
    result = root / path
    if not result.resolve(strict=True).is_relative_to(root.resolve()):
        raise ValueError('native notice path escapes package')
    if not result.is_file():
        raise ValueError('native notice is not a regular file')
    return result


def selected_files(root):
    """Include extensionless helpers and modules, not just names ending in .so."""
    result, count, total = {}, 0, 0
    pending = [root]
    while pending:
        with os.scandir(pending.pop()) as entries:
            for entry in entries:
                count += 1
                if count > 50000:
                    raise ValueError('native inventory exceeds entry bound')
                path = Path(entry.path)
                if entry.is_symlink():
                    if not path.resolve(strict=True).is_relative_to(root):
                        raise ValueError('native package symlink escapes package')
                    continue
                if entry.is_dir(follow_symlinks=False):
                    pending.append(path)
                    continue
                if not entry.is_file(follow_symlinks=False):
                    raise ValueError('special file in native package')
                total += entry.stat().st_size
                if total > 4 * 1024**3:
                    raise ValueError('native inventory exceeds byte bound')
                name = path.relative_to(root).as_posix()
                with path.open('rb') as stream:
                    elf = stream.read(4) == b'\x7fELF'
                if elf or name in HELPERS:
                    result[name] = sha(path)
    if len(result) > 1024:
        raise ValueError('native component count exceeds bound')
    return result


def command(args, accepted=(0,)):
    # Tools are trusted build dependencies. Timeout and output bounds prevent
    # an unexpected package database or tool response growing the manifest.
    result = subprocess.run(args, capture_output=True, text=True, timeout=60,
                            env={**os.environ, 'LC_ALL': 'C'})
    if result.returncode not in accepted:
        raise ValueError(f'native provenance command failed: {args[0]}: {result.stderr[:500]}')
    if len(result.stdout) > 8 * 1024**2:
        raise ValueError('native provenance command exceeds output bound')
    return result.stdout


def build_id(path):
    result = command(['readelf', '-n', str(path)])
    ids = set(re.findall(r'Build ID: ([0-9a-f]+)', result))
    if len(ids) != 1:
        raise ValueError(f'missing or ambiguous GNU build ID: {path.name}')
    return ids.pop()


class DpkgProvenance:
    def __init__(self):
        self.packages = {}
        self.ids = {}
        self.hashes = {}

    def identify(self, path, digest):
        # Query both merged-/usr and older /lib database spellings by basename.
        # SONAME copies can point to a differently named original package file.
        if not re.fullmatch(r'[A-Za-z0-9_.+-]+', path.name):
            raise ValueError('unreviewed native filename')
        candidates = command(['dpkg-query', '--search', '*/' + path.name], (0, 1))
        elf = path.open('rb')
        with elf:
            is_elf = elf.read(4) == b'\x7fELF'
        identity = build_id(path) if is_elf else None
        matches = {}
        for line in candidates.splitlines():
            if ': ' not in line:
                continue
            owner, source = line.rsplit(': ', 1)
            if not re.fullmatch(r'[a-z0-9][a-z0-9+.-]*(?::[a-z0-9-]+)?', owner):
                continue
            original = Path(source)
            if not original.is_file() or not original.is_absolute():
                continue
            original = original.resolve()
            if is_elf:
                if original not in self.ids:
                    self.ids[original] = build_id(original)
                same = self.ids[original] == identity
            else:
                same = sha(original) == digest
            if same:
                # A development-package SONAME symlink belongs to the runtime
                # file's owner, whose copyright and version we must preserve.
                owners = command(['dpkg-query', '--search', str(original)], (0, 1))
                if not owners.strip() and str(original).startswith('/usr/'):
                    owners = command(['dpkg-query', '--search', str(original)[4:]], (0, 1))
                for owned in owners.splitlines():
                    if ': ' not in owned:
                        continue
                    package, owned_path = owned.rsplit(': ', 1)
                    if Path(owned_path).resolve() == original and re.fullmatch(
                            r'[a-z0-9][a-z0-9+.-]*(?::[a-z0-9-]+)?', package):
                        matches[(package, str(original))] = original
        if not matches:
            raise ValueError(f'no distro provenance for {path.name}')
        if len({p for p, _ in matches}) != 1:
            raise ValueError(f'ambiguous distro provenance for {path.name}')
        package, source = sorted(matches)[0]
        original = Path(source)
        if original not in self.hashes:
            self.hashes[original] = sha(original)
        if package not in self.packages:
            fields = command(['dpkg-query', '--show', '--showformat=${binary:Package}\t${Version}\t${source:Package}\t${source:Version}', package]).split('\t')
            if len(fields) != 4 or not all(fields):
                raise ValueError('incomplete distro package metadata')
            self.packages[package] = dict(zip(('package', 'version', 'source_package', 'source_version'), fields))
        return {**self.packages[package], 'original_path': source,
                'original_sha256': self.hashes[original], 'build_id': identity}

    def copyright(self, package):
        name = package.split(':')[0]
        path = Path('/usr/share/doc') / name / 'copyright'
        if not path.resolve(strict=True).is_relative_to('/usr/share/doc'):
            raise ValueError('distro copyright escapes documentation root')
        return path

    def common_license(self, name):
        path = Path('/usr/share/common-licenses') / name
        if not path.resolve(strict=True).is_relative_to('/usr/share/common-licenses'):
            raise ValueError('common license escapes documentation root')
        return path


def collect(root, provenance=None):
    root = root.resolve(strict=True)
    provenance = provenance or DpkgProvenance()
    selected = selected_files(root)
    if not FIRST_PARTY.issubset(selected):
        raise ValueError('AppDir lacks expected first-party binaries')
    components, packages, notices = [], {}, {}
    total = 0

    def copy_notice(source, destination):
        nonlocal total
        if destination in notices:
            return
        size = source.stat().st_size
        if not 0 < size <= MAX_FILE or total + size > MAX_TOTAL:
            raise ValueError('native copyright exceeds bounds')
        data = source.read_bytes()
        total += len(data)
        target = root / destination
        # Never follow an existing generated destination outside the AppDir.
        if not target.resolve().is_relative_to(root):
            raise ValueError('native notice destination escapes package')
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
        notices[destination] = {'path': destination, 'sha256': hashlib.sha256(data).hexdigest(), 'bytes': len(data)}
        for common in sorted(set(re.findall(rb'/usr/share/common-licenses/([A-Za-z0-9.+-]+)', data))):
            name = common.decode('ascii').rstrip('.')
            copy_notice(provenance.common_license(name), f'{NOTICE_ROOT}/common/{name}')

    for name, digest in sorted(selected.items()):
        entry = {'path': name, 'sha256': digest}
        if name in FIRST_PARTY:
            entry['kind'] = 'first-party'
        elif name in {'AppRun', 'AppRun.wrapped'} and digest in APPRUN:
            entry.update(kind='apprun', architecture=APPRUN[digest],
                         origin='https://github.com/tauri-apps/binary-releases/releases/tag/apprun-old')
        else:
            metadata = provenance.identify(root / name, digest)
            package = metadata['package']
            copyright_path = f'{NOTICE_ROOT}/packages/{package}/copyright'
            copy_notice(provenance.copyright(package), copyright_path)
            packages[package] = {k: metadata[k] for k in ('package', 'version', 'source_package', 'source_version')}
            packages[package].update(copyright=copyright_path,
                source_locator='https://launchpad.net/ubuntu/+source/' + quote(metadata['source_package'], safe='') + '/' + quote(metadata['source_version'], safe=''))
            entry.update(kind='distro', **metadata)
        components.append(entry)
    result = {'schema': 1, 'components': components,
              'packages': sorted(packages.values(), key=lambda p: p['package']),
              'notices': sorted(notices.values(), key=lambda p: p['path']),
              'scope': 'Selected distro ELF files and copied xdg helpers; original copyright and referenced common-license texts.',
              'remaining_review': ['AppImage launcher and outer runtime provenance/source obligations',
                                   'Per-package redistribution and corresponding-source obligations; source locators are not a source offer']}
    destination = root / MANIFEST
    if not destination.resolve().is_relative_to(root):
        raise ValueError('native manifest destination escapes package')
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    verify(root)
    return result


def verify(root):
    root = root.resolve(strict=True)
    manifest = safe_file(root, MANIFEST)
    if manifest.stat().st_size > 2 * 1024**2:
        raise ValueError('native manifest exceeds bound')
    data = json.loads(manifest.read_text(encoding='utf-8'))
    if data.get('schema') != 1 or not data.get('packages') or not data.get('notices'):
        raise ValueError('incomplete native notice manifest')
    actual = selected_files(root)
    declared = {item['path']: item['sha256'] for item in data['components']}
    if len(declared) != len(data['components']) or declared != actual:
        raise ValueError('native component inventory differs from package')
    packages = {p['package']: p for p in data['packages']}
    notices = {p['path']: p for p in data['notices']}
    if len(packages) != len(data['packages']) or len(notices) != len(data['notices']):
        raise ValueError('duplicate native provenance entry')
    total = 0
    for name, item in notices.items():
        if not name.startswith(NOTICE_ROOT + '/'):
            raise ValueError('native notice outside notice directory')
        path = safe_file(root, name)
        size = path.stat().st_size
        total += size
        if not 0 < size <= MAX_FILE or total > MAX_TOTAL:
            raise ValueError('native copyright exceeds bounds')
        if size != item['bytes'] or sha(path) != item['sha256']:
            raise ValueError('native copyright differs from manifest')
    observed = set()
    for item in data['components']:
        name = item['path']
        if item['kind'] == 'first-party':
            if name not in FIRST_PARTY:
                raise ValueError('unexpected first-party exemption')
        elif item['kind'] == 'apprun':
            if name not in {'AppRun', 'AppRun.wrapped'} or item['sha256'] not in APPRUN:
                raise ValueError('unreviewed AppRun exemption')
        elif item['kind'] == 'distro':
            p = packages[item['package']]
            if p['copyright'] not in notices or any(item[k] != p[k] for k in ('version', 'source_package', 'source_version')):
                raise ValueError('native package attribution differs')
            observed.add(item['package'])
        else:
            raise ValueError('unknown native component kind')
    if observed != set(packages) or not FIRST_PARTY.issubset(declared):
        raise ValueError('native package provenance is incomplete')
    return {'components': len(actual), 'distro_packages': len(packages), 'notice_files': len(notices),
            'manifest_sha256': sha(manifest), 'remaining_review': data['remaining_review']}
