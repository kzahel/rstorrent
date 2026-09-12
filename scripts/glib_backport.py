"""Verify the one approved GLib source backport before build/release review."""
import hashlib
import json
from pathlib import Path
import tomllib

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = 'distribution/patches/glib-0.18.5-source.json'
MANIFEST_SHA256 = 'b4c7d59e738c7999884177232c056720ddeb15b1e83568594cc60ba4b7e7fa0b'


def sha(data):
    return hashlib.sha256(data).hexdigest()


def read_regular(path, limit=4 * 1024 * 1024):
    if path.is_symlink() or not path.is_file() or path.stat().st_size > limit:
        raise ValueError(f'invalid backport file: {path.name}')
    return path.read_bytes()


def verify(root=ROOT):
    raw = read_regular(root / MANIFEST)
    if sha(raw) != MANIFEST_SHA256:
        raise ValueError('GLib provenance manifest changed; explicit source review required')
    source = json.loads(raw)
    vendor = root / source['vendor_path']
    if vendor.is_symlink() or (root / 'vendor').is_symlink():
        raise ValueError('GLib vendor directory must not be a symlink')
    expected = source['upstream_files'].copy()
    expected[source['patched_file']] = source['patched_file_sha256']
    actual = {}
    for path in vendor.rglob('*'):
        if path.is_symlink():
            raise ValueError('GLib source symlinks are not permitted')
        if path.is_dir():
            continue
        if len(actual) >= 256:
            raise ValueError('GLib source inventory exceeds bound')
        actual[path.relative_to(vendor).as_posix()] = sha(read_regular(path))
    if actual != expected:
        raise ValueError('GLib source inventory differs from the approved backport')
    if sha(read_regular(root / source['patch'])) != source['patch_sha256']:
        raise ValueError('GLib patch provenance changed')
    # Independently bind the two-line delta to the original published file.
    modified = read_regular(vendor / source['patched_file'])
    original = modified.replace(b'let mut p: *mut libc::c_char = std::ptr::null_mut();',
                                b'let p: *mut libc::c_char = std::ptr::null_mut();')
    original = original.replace(b'                &mut p,', b'                &p,')
    if sha(original) != source['upstream_files'][source['patched_file']]:
        raise ValueError('GLib source is not the exact two-line backport')
    workspace = tomllib.loads(read_regular(root / 'Cargo.toml').decode())
    if workspace.get('patch', {}).get('crates-io', {}).get('glib') != {'path': source['vendor_path']}:
        raise ValueError('Cargo must select the reviewed GLib path override')
    locked = tomllib.loads(read_regular(root / 'Cargo.lock').decode())
    packages = [p for p in locked['package'] if p['name'] == 'glib']
    if (len(packages) != 1 or packages[0]['version'] != source['version']
            or 'source' in packages[0] or 'checksum' in packages[0]):
        raise ValueError('Cargo.lock must select exactly one local GLib 0.18.5')
    return {key: source[key] for key in (
        'package', 'version', 'advisory', 'license', 'upstream_archive',
        'upstream_archive_sha256', 'upstream_revision', 'patch', 'patch_sha256',
        'patched_file_sha256')} | {'source_manifest_sha256': MANIFEST_SHA256,
                                   'status': 'source-verified-backport'}


def audit_lock(root=ROOT):
    proof = verify(root)
    original = read_regular(root / 'Cargo.lock')
    text = original.decode().replace('\r\n', '\n')
    marker = 'name = "glib"\nversion = "0.18.5"\n'
    if text.count(marker) != 1:
        raise ValueError('ambiguous GLib audit identity')
    replacement = (marker + 'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
                   + f'checksum = "{proof["upstream_archive_sha256"]}"\n')
    projected = text.replace(marker, replacement).encode()
    # The audit view differs only in this package's registry identity.
    before = tomllib.loads(text)
    after = tomllib.loads(projected.decode())
    glib = next(p for p in after['package'] if p['name'] == 'glib')
    del glib['source'], glib['checksum']
    if after != before:
        raise ValueError('audit projection changed unrelated dependency metadata')
    return projected, {'cargo_lock_sha256': sha(original),
                       'audit_lock_sha256': sha(projected),
                       'backport_source_manifest_sha256': MANIFEST_SHA256}


if __name__ == '__main__':
    print(json.dumps(verify(), indent=2))
