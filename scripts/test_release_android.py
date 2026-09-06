import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('release_android', Path(__file__).with_name('release-android.py'))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    source = '    versionCode = 9\n    versionName = "0.1.9"\n'

    def test_bump_preserves_source_and_increments_code(self):
        text, code = release.bump(self.source + '// retained\n', '0.2.0')
        self.assertEqual(release.read_version(text), ('0.2.0', 10))
        self.assertEqual(code, 10)
        self.assertTrue(text.endswith('// retained\n'))

    def test_rejects_unsafe_or_ambiguous_versions(self):
        for version in ('v1.0.0', '1.2', '01.2.3', '1.2.3\n', '1.2.3;echo bad', '0.1.9', '0.1.8'):
            with self.subTest(version=version), self.assertRaises(ValueError):
                release.bump(self.source, version)

    def test_exhausted_code_and_duplicate_fields(self):
        for text in (self.source.replace('= 9', '= 2100000000'), self.source * 2):
            with self.assertRaises(ValueError):
                release.bump(text, '1.0.0')

    def test_notes_are_exact_and_bounded_by_next_release(self):
        self.assertEqual(release.release_notes('## [1.2.3]\n\n- Good\n\n## [1.2.2]\nOld', '1.2.3'), '- Good')
        for text in ('## [1x2x3]\nwrong', '## [1.2.3]\n', '## [1.2.3]\na\n## [1.2.3]\nb'):
            with self.assertRaises(ValueError):
                release.release_notes(text, '1.2.3')


class ReleaseIntegrationTests(unittest.TestCase):
    def test_dry_run_and_atomic_release_to_local_remote(self):
        import os
        import shutil
        import subprocess
        import tempfile
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / 'checkout'
            root.mkdir()
            remote = Path(directory) / 'remote.git'
            env = os.environ | {'GIT_CONFIG_NOSYSTEM': '1', 'GIT_CONFIG_GLOBAL': os.devnull}

            def run(*args):
                return subprocess.check_output(args, cwd=root, env=env, text=True, stderr=subprocess.STDOUT)

            run('git', 'init', '-b', 'main')
            run('git', 'config', 'user.name', 'Release Tester')
            run('git', 'config', 'user.email', 'release@localhost')
            run('git', 'init', '--bare', str(remote))
            run('git', 'remote', 'add', 'origin', str(remote))
            (root / 'scripts').mkdir()
            shutil.copy(Path(__file__).with_name('release-android.py'), root / 'scripts/release-android.py')
            (root / release.GRADLE).parent.mkdir(parents=True)
            (root / release.GRADLE).write_text(ReleaseTests.source)
            (root / release.CHANGELOG).write_text('## [0.2.0]\n\n- Test release\n')
            run('git', 'add', '.')
            run('git', 'commit', '-m', 'Initial fixture')
            before = run('git', 'rev-parse', 'HEAD')
            run('python3', 'scripts/release-android.py', '0.2.0', '--dry-run')
            self.assertEqual(run('git', 'rev-parse', 'HEAD'), before)
            self.assertEqual(run('git', 'status', '--porcelain'), '')
            run('python3', 'scripts/release-android.py', '0.2.0')
            self.assertEqual(release.read_version((root / release.GRADLE).read_text()), ('0.2.0', 10))
            self.assertIn('refs/tags/android-v0.2.0', run('git', 'ls-remote', '--tags', 'origin'))
            self.assertIn('Test release', run('python3', 'scripts/release-android.py', '--check', '--tag', 'android-v0.2.0'))
            with self.assertRaises(subprocess.CalledProcessError):
                run('python3', 'scripts/release-android.py', '--check', '--tag', 'android-v0.2.1')


if __name__ == '__main__':
    unittest.main()
