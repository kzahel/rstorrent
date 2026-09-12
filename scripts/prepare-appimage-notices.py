#!/usr/bin/env python3
"""Install the project-local, pre-signing AppImage attribution output hook."""
import hashlib
import json
import os
from pathlib import Path
import platform
import shlex
import subprocess
import sys
import tempfile
import urllib.request

from native_notices import sha

ROOT = Path(__file__).resolve().parents[1]
# Upstream release revision 536b068787179ea901964bd7dabc7bf61e4941c3.
# The continuous URL may move; the digest must be reviewed before updating.
PLUGINS = {
    'x86_64': '0441769ab38009504d2678c38cd7e526955388dd30a215b4a20afaa5471652f2',
    'aarch64': 'ce574719bcf9cc1fb12728d60b17e48cc87d9b6c40f6f48b04cff7d273b5eb24',
}


def download_plugin(destination, arch):
    expected = PLUGINS[arch]
    if destination.is_file() and destination.stat().st_size <= 128 * 1024**2 and sha(destination) == expected:
        return
    url = f'https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/continuous/linuxdeploy-plugin-appimage-{arch}.AppImage'
    with tempfile.TemporaryDirectory(dir=destination.parent, prefix='notice-plugin-') as temp:
        path = Path(temp) / 'plugin'
        digest, total = hashlib.sha256(), 0
        with urllib.request.urlopen(url, timeout=60) as response, path.open('wb') as output:
            while chunk := response.read(65536):
                total += len(chunk)
                if total > 128 * 1024**2:
                    raise ValueError('AppImage output plugin exceeds download bound')
                digest.update(chunk)
                output.write(chunk)
        if digest.hexdigest() != expected:
            raise ValueError('AppImage output plugin digest changed; review the upstream asset')
        path.chmod(0o755)
        path.replace(destination)


def main():
    if sys.platform != 'linux':
        return
    if platform.freedesktop_os_release().get('ID') != 'ubuntu':
        raise ValueError('native notice source locators currently require an Ubuntu builder')
    arch = platform.machine()
    if arch not in PLUGINS:
        raise ValueError('unreviewed native AppImage architecture')
    locked = json.loads((ROOT / 'clients/web/package-lock.json').read_text())
    if locked['packages']['node_modules/@tauri-apps/cli']['version'] != '2.11.4':
        raise ValueError('review the AppImage hook against the new Tauri CLI')
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--no-deps', '--format-version', '1', '--locked'], cwd=ROOT, timeout=60))
    directory = Path(metadata['target_directory']) / '.tauri'
    directory.mkdir(parents=True, exist_ok=True)
    delegate = directory / f'rstorrent-output-appimage-{arch}.AppImage'
    download_plugin(delegate, arch)
    # Tauri uses this exact external plugin filename and preserves existing
    # files. This wrapper is build-local; absolute paths never enter notices.
    wrapper = directory / 'linuxdeploy-plugin-appimage.AppImage'
    script = ROOT / 'scripts/appimage-notice-plugin.py'
    wrapper.write_text('#!/bin/sh\nexec ' + shlex.join([sys.executable, str(script), '--delegate', str(delegate)]) + ' "$@"\n')
    wrapper.chmod(0o755)
    print('Prepared the project-local AppImage native notice hook')


if __name__ == '__main__':
    main()
