#!/usr/bin/env python3
"""Check Android notice assets and their native dependency inventory."""
import argparse
import hashlib
import json
from pathlib import Path
import zipfile

ABIS = {'arm64-v8a', 'x86_64'}
TARGETS = ['aarch64-linux-android', 'x86_64-linux-android']


def inspect(archive, expected_variant, export_directory=None):
    with zipfile.ZipFile(archive) as package:
        names = package.namelist()
        if len(names) > 100000 or len(names) != len(set(names)):
            raise ValueError('oversized or duplicate Android package inventory')
        base = 'base/' if archive.suffix == '.aab' else ''
        prefix = base + 'assets/notices/'
        manifest_entry = package.getinfo(prefix + 'dependency-manifest.json')
        text_entry = package.getinfo(prefix + 'THIRD_PARTY_NOTICES.txt')
        if manifest_entry.file_size > 2 * 1024**2 or text_entry.file_size > 16 * 1024**2:
            raise ValueError('Android notices exceed package bounds')
        manifest = json.loads(package.read(manifest_entry))
        text = package.read(text_entry)
        if (manifest.get('schema') != 1 or manifest.get('variant') != expected_variant or
                manifest.get('targets') != TARGETS or not manifest.get('rust') or not manifest.get('maven')):
            raise ValueError('incomplete or wrong-variant Android notice manifest')
        if hashlib.sha256(text).hexdigest() != manifest['notices_sha256']:
            raise ValueError('Android notice content differs from manifest')
        declared = set()
        identities = set()
        for entry in manifest['maven']:
            identity = (entry['component'], entry['artifact_file'])
            if identity in identities or not entry.get('pom_evidence') or not entry.get('selected_license'):
                raise ValueError('incomplete or duplicate Maven provenance')
            identities.add(identity)
            for native in entry['native_libraries']:
                name = native['path']
                if name in declared or name.split('/')[0] not in ABIS:
                    raise ValueError('duplicate or unexpected native attribution')
                declared.add(name)
        for abi in ABIS:
            declared.add(f'{abi}/librstorrent_android.so')
        actual = {n[len(base + 'lib/'):] for n in names if n.startswith(base + 'lib/') and n.endswith('.so')}
        if actual != declared:
            raise ValueError('packaged native libraries differ from attribution inventory')
        if export_directory is not None:
            export_directory.mkdir(parents=True, exist_ok=True)
            (export_directory / 'dependency-manifest.json').write_bytes(package.read(manifest_entry))
            (export_directory / 'THIRD_PARTY_NOTICES.txt').write_bytes(text)
        return {'schema': 1, 'variant': expected_variant, 'maven_artifacts': len(identities),
                'rust_packages': len(manifest['rust']), 'native_libraries': len(actual),
                'notices_sha256': manifest['notices_sha256'],
                'manifest_sha256': hashlib.sha256(package.read(manifest_entry)).hexdigest(),
                'scope': 'Notice asset integrity and exact native library names; published artifact hashes identify pre-AGP inputs, not stripped APK bytes'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive', type=Path, required=True)
    parser.add_argument('--variant', choices=('debug', 'release'), required=True)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--export-notices', type=Path,
                        help='retain exact verified notice assets from the final archive')
    args = parser.parse_args()
    result = inspect(args.archive, args.variant, args.export_notices)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
