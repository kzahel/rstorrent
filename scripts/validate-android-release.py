#!/usr/bin/env python3
"""Check signed Android outputs before making them downloadable."""
import argparse
import base64
import hashlib
import importlib.util
import os
from pathlib import Path
import re
import struct
import subprocess
import xml.etree.ElementTree as ET
import zipfile

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('release_android', ROOT / 'scripts/release-android.py')
release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release)
ANDROID = '{http://schemas.android.com/apk/res/android}'


def run(*args):
    return subprocess.check_output(list(map(str, args)), text=True, stderr=subprocess.STDOUT)


def check_manifest(text, version, code):
    root = ET.fromstring(text)
    assert root.get('package') == 'com.jstorrent.rstorrent', 'Wrong package'
    assert root.get(ANDROID + 'versionName') == version, 'Wrong versionName'
    assert root.get(ANDROID + 'versionCode') == str(code), 'Wrong versionCode'
    sdk = root.find('uses-sdk')
    assert sdk.get(ANDROID + 'targetSdkVersion') == '36', 'Wrong target SDK'
    assert sdk.get(ANDROID + 'minSdkVersion') == '28', 'Wrong minimum SDK'
    app = root.find('application')
    assert app.get(ANDROID + 'debuggable', 'false') == 'false', 'Debuggable release'
    for item in app.iter():
        name = item.get(ANDROID + 'name', '')
        assert not name.endswith(('CommandReceiver', 'ProductTestReceiver')), 'Diagnostic receiver in release'


def check_elf(data, name):
    assert data[:6] == b'\x7fELF\x02\x01', f'{name}: expected little-endian ELF64'
    offset = struct.unpack_from('<Q', data, 32)[0]
    size, count = struct.unpack_from('<HH', data, 54)
    loads = 0
    for index in range(count):
        header = offset + index * size
        if struct.unpack_from('<I', data, header)[0] == 1:
            loads += 1
            alignment = struct.unpack_from('<Q', data, header + 48)[0]
            assert alignment >= 16384 and alignment & (alignment - 1) == 0, f'{name}: not 16 KiB aligned'
    assert loads, f'{name}: missing load segments'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundletool', type=Path, required=True)
    args = parser.parse_args()
    assert hashlib.sha256(args.bundletool.read_bytes()).hexdigest() == 'a099cfa1543f55593bc2ed16a70a7c67fe54b1747bb7301f37fdfd6d91028e29', 'Unexpected bundletool checksum'
    sdk = Path(os.environ.get('ANDROID_HOME') or os.environ['ANDROID_SDK_ROOT'])
    tools = sdk / 'build-tools/35.0.0'
    apk = ROOT / 'clients/android/app/build/outputs/apk/release/app-release.apk'
    aab = ROOT / 'clients/android/app/build/outputs/bundle/release/app-release.aab'
    version, code = release.read_version((ROOT / release.GRADLE).read_text())
    cert = (ROOT / 'clients/android/upload-certificate.pem').read_text()
    cert = base64.b64decode(''.join(cert.splitlines()[1:-1]))
    fingerprint = hashlib.sha256(cert).hexdigest()
    signatures = run(tools / 'apksigner', 'verify', '--verbose', '--print-certs', apk)
    assert f'Signer #1 certificate SHA-256 digest: {fingerprint}' in signatures, 'Unexpected APK signer'
    result = run('jarsigner', '-verify', aab)
    assert 'jar verified.' in result, 'AAB signature verification failed'
    result = run('keytool', '-printcert', '-jarfile', aab)
    assert fingerprint.upper() in result.replace(':', ''), 'Unexpected AAB signer'
    run(tools / 'zipalign', '-c', '-P', '16', '4', apk)
    apk_manifest = run(sdk / 'cmdline-tools/latest/bin/apkanalyzer', 'manifest', 'print', apk)
    aab_manifest = run('java', '-jar', args.bundletool, 'dump', 'manifest', f'--bundle={aab}')
    check_manifest(apk_manifest, version, code)
    check_manifest(aab_manifest, version, code)
    config = run('java', '-jar', args.bundletool, 'dump', 'config', f'--bundle={aab}')
    assert 'PAGE_ALIGNMENT_16K' in config, 'AAB must request 16 KiB ZIP alignment'
    run('java', '-jar', args.bundletool, 'validate', f'--bundle={aab}')
    native = []
    for archive, prefix in ((apk, 'lib/'), (aab, 'base/lib/')):
        with zipfile.ZipFile(archive) as z:
            libs = {n[len(prefix):]: z.read(n) for n in z.namelist() if n.startswith(prefix) and n.endswith('.so')}
            assert {n.split('/')[0] for n in libs} == {'arm64-v8a', 'x86_64'}, 'Unexpected packaged ABIs'
            for abi in ('arm64-v8a', 'x86_64'):
                assert f'{abi}/librstorrent_android.so' in libs, 'Missing Rust library'
            for name, data in libs.items():
                check_elf(data, name)
            native.append(libs)
    assert native[0] == native[1], 'APK and AAB native libraries differ'
    print(f'Validated {version} ({code}): signed APK/AAB, canary manifest, both ABIs, 16 KiB ELF/ZIP alignment')


if __name__ == '__main__':
    main()
