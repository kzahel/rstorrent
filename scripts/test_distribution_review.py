import copy
from datetime import date
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace('-', '_'), Path(__file__).with_name(name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


audit = load('review-dependency-audit')
package = load('inspect-distribution')
notices = load('prepare-release-notices')


class AdvisoryReviewTests(unittest.TestCase):
    def setUp(self):
        self.cargo = {'vulnerabilities': {'count': 0},
                      'database': {'last-commit': 'abc', 'last-updated': '2026-09-09T00:00:00+00:00'},
                      'warnings': {'unsound': [{'advisory': {'id': 'RUSTSEC-test'},
                                               'package': {'name': 'example', 'version': '1.0'}}]}}
        self.npm = {'metadata': {'vulnerabilities': {'total': 0}}}
        self.policy = {'review_expires': '2026-10-12',
                       'warnings': [{'kind': 'unsound', 'advisory': 'RUSTSEC-test',
                                     'package': 'example', 'version': '1.0'}],
                       'release_blockers': ['unresolved unsound implementation']}

    def test_review_never_turns_known_blocker_into_release_clearance(self):
        result = audit.review(self.cargo, self.npm, self.policy, date(2026, 9, 12))
        self.assertFalse(result['release_ready'])
        self.assertEqual(len(result['reviewed_warnings']), 1)

    def test_new_and_removed_warnings_require_review(self):
        changed = copy.deepcopy(self.cargo)
        changed['warnings'] = {}
        with self.assertRaisesRegex(ValueError, 'inventory changed'):
            audit.review(changed, self.npm, self.policy, date(2026, 9, 12))
        changed['warnings'] = {'yanked': [{'package': {'name': 'new', 'version': '1'}}]}
        with self.assertRaisesRegex(ValueError, 'inventory changed'):
            audit.review(changed, self.npm, self.policy, date(2026, 9, 12))

    def test_expired_review_stale_database_and_vulnerabilities_fail(self):
        with self.assertRaisesRegex(ValueError, '30 days'):
            audit.review(self.cargo, self.npm, self.policy, date(2026, 10, 13))
        self.cargo['database']['last-updated'] = '2026-10-12T00:00:00+00:00'
        with self.assertRaisesRegex(ValueError, 'expired'):
            audit.review(self.cargo, self.npm, self.policy, date(2026, 10, 13))
        self.cargo['vulnerabilities']['count'] = 1
        with self.assertRaisesRegex(ValueError, 'vulnerabilities'):
            audit.review(self.cargo, self.npm, self.policy, date(2026, 9, 12))
        self.cargo['vulnerabilities']['count'] = 0
        self.npm['metadata']['vulnerabilities']['total'] = 1
        with self.assertRaisesRegex(ValueError, 'npm'):
            audit.review(self.cargo, self.npm, self.policy, date(2026, 9, 12))


class DistributionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def test_new_package_requires_notices_and_validates_content(self):
        with self.assertRaisesRegex(ValueError, 'exactly one'):
            package.inspect(self.root)
        (self.root / 'THIRD_PARTY_NOTICES.txt').write_text('original attribution')
        manifest = {'schema': 1, 'target': 'test', 'rust': [{}], 'npm': [{}],
                    'cargo_lock_sha256': 'a', 'npm_lock_sha256': 'b',
                    'notices_sha256': notices.digest(b'original attribution')}
        (self.root / 'dependency-manifest.json').write_text(json.dumps(manifest))
        self.assertEqual(len(package.inspect(self.root)['notice_manifests']), 1)
        (self.root / 'THIRD_PARTY_NOTICES.txt').write_text('changed attribution')
        with self.assertRaisesRegex(ValueError, 'differs'):
            package.inspect(self.root)

    def test_historical_inventory_is_explicitly_not_notice_clearance(self):
        self.assertEqual(package.inspect(self.root, False)['notice_manifests'], [])

    def test_private_key_crossing_read_boundary_is_rejected(self):
        (self.root / 'settings.json').write_bytes(b' ' * 65530 + b'-----BEGIN PRIVATE KEY-----')
        with self.assertRaisesRegex(ValueError, 'private key'):
            package.inspect(self.root, False)

    def test_development_files_and_endpoints_are_rejected(self):
        for name, data in [('.env.production', b'anything'), ('app.js.map', b'{}'),
                           ('index.html', b'http://localhost:5173/@vite/client')]:
            with self.subTest(name=name):
                path = self.root / name
                path.write_bytes(data)
                with self.assertRaises(ValueError):
                    package.inspect(self.root, False)
                path.unlink()

    def test_external_symlink_is_rejected(self):
        (self.root / 'escape').symlink_to(self.root.parent, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, 'escapes'):
            package.inspect(self.root, False)

    def test_unknown_and_changed_license_grants_fail_closed(self):
        p = {'name': 'example', 'version': '1.0', 'license': 'MIT'}
        (self.root / 'Cargo.toml').write_text('original grant')
        with self.assertRaisesRegex(ValueError, 'missing reviewed'):
            notices.supplemental(p, self.root, {})
        catalog = {'example@1.0': {'license': 'MIT', 'manifest_sha256': notices.digest(b'original grant'),
                                  'files': [], 'declaration_only': True}}
        self.assertIn('declaration', notices.supplemental(p, self.root, catalog)[0])
        (self.root / 'Cargo.toml').write_text('changed grant')
        with self.assertRaisesRegex(ValueError, 'drift'):
            notices.supplemental(p, self.root, catalog)


class NativeNoticeTests(unittest.TestCase):
    def setUp(self):
        import native_notices
        self.native = native_notices
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve() / 'AppDir'
        self.root.mkdir()
        for name in native_notices.FIRST_PARTY | {'usr/lib/libexample.so.1', 'usr/bin/xdg-mime'}:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b'#!/bin/sh\n' if name.endswith('xdg-mime') else b'\x7fELFexample')
        self.copyright = self.root.parent / 'copyright'
        self.copyright.write_bytes(b'Original holder\nSee /usr/share/common-licenses/MIT\n')
        self.common = self.root.parent / 'MIT'
        self.common.write_bytes(b'Original common license\n')
        test = self

        class Provenance:
            def identify(self, path, digest):
                return {'package': 'example:amd64', 'version': '1.2-1',
                        'source_package': 'example', 'source_version': '1.2-1',
                        'original_path': '/usr/lib/libexample.so.1.2',
                        'original_sha256': digest, 'build_id': 'abcdef'}

            def copyright(self, package):
                return test.copyright

            def common_license(self, name):
                return test.common

        self.provenance = Provenance()

    def test_selected_components_and_original_license_bytes_round_trip(self):
        result = self.native.collect(self.root, self.provenance)
        self.assertEqual(len(result['components']), 4)
        self.assertEqual(len(result['packages']), 1)
        self.assertEqual(len(result['notices']), 2)
        copyright_path = self.root / result['packages'][0]['copyright']
        self.assertEqual(copyright_path.read_bytes(), self.copyright.read_bytes())
        self.assertEqual(self.native.verify(self.root)['distro_packages'], 1)
        self.assertNotIn(str(self.root.parent), json.dumps(result))
        self.assertTrue(result['remaining_review'])

    def test_missing_or_changed_component_is_rejected(self):
        self.native.collect(self.root, self.provenance)
        path = self.root / 'usr/lib/libexample.so.1'
        original = path.read_bytes()
        for changed in (b'\x7fELFmodified', None):
            with self.subTest(changed=changed):
                if changed is None:
                    path.unlink()
                else:
                    path.write_bytes(changed)
                with self.assertRaisesRegex(ValueError, 'inventory differs'):
                    self.native.verify(self.root)
                path.write_bytes(original)
        (self.root / 'usr/lib/unattributed').write_bytes(b'\x7fELFnew')
        with self.assertRaisesRegex(ValueError, 'inventory differs'):
            self.native.verify(self.root)

    def test_corrupted_notice_and_first_party_exemption_are_rejected(self):
        result = self.native.collect(self.root, self.provenance)
        path = self.root / result['packages'][0]['copyright']
        path.write_bytes(b'changed')
        with self.assertRaisesRegex(ValueError, 'differs'):
            self.native.verify(self.root)
        path.write_bytes(self.copyright.read_bytes())
        next(item for item in result['components'] if item['path'].endswith('libexample.so.1'))['kind'] = 'first-party'
        (self.root / self.native.MANIFEST).write_text(json.dumps(result))
        with self.assertRaisesRegex(ValueError, 'exemption'):
            self.native.verify(self.root)

    def test_external_symlink_and_oversized_notice_are_rejected(self):
        (self.root / 'escape').symlink_to(self.root.parent)
        with self.assertRaisesRegex(ValueError, 'escapes'):
            self.native.collect(self.root, self.provenance)
        (self.root / 'escape').unlink()
        from unittest.mock import patch
        with patch.object(self.native, 'MAX_FILE', 2):
            with self.assertRaisesRegex(ValueError, 'bounds'):
                self.native.collect(self.root, self.provenance)

    def test_unmapped_distro_binary_fails_before_packaging(self):
        from unittest.mock import patch
        with patch.object(self.provenance, 'identify', side_effect=ValueError('no distro provenance')):
            with self.assertRaisesRegex(ValueError, 'no distro provenance'):
                self.native.collect(self.root, self.provenance)

    def test_dpkg_matches_runtime_owner_and_rejects_ambiguity(self):
        from unittest.mock import patch
        source = self.root.parent / 'libexample.so.1'
        source.write_bytes(b'\x7fELFsource')
        calls = []

        def command(args, accepted=(0,)):
            calls.append(args)
            if args[0] == 'readelf':
                return 'Build ID: abcdef\n'
            if '--show' in args:
                return 'example:amd64\t1.2-1\texample\t1.2-1'
            return f'example:amd64: {source}\n'

        with patch.object(self.native, 'command', side_effect=command):
            result = self.native.DpkgProvenance().identify(source, self.native.sha(source))
        self.assertEqual(result['package'], 'example:amd64')
        self.assertEqual(result['build_id'], 'abcdef')
        self.assertTrue(any(a[-1] == str(source) for a in calls if '--search' in a))
        def ambiguous(args, accepted=(0,)):
            output = command(args, accepted)
            if '--search' in args:
                output += f'another:amd64: {source}\n'
            return output
        with patch.object(self.native, 'command', side_effect=ambiguous):
            with self.assertRaisesRegex(ValueError, 'ambiguous distro'):
                self.native.DpkgProvenance().identify(source, self.native.sha(source))

    def test_explicit_appimage_gate_cannot_be_bypassed_by_missing_apprun(self):
        with self.assertRaises(FileNotFoundError):
            package.inspect(self.root, require_notices=False, require_native=True)


if __name__ == '__main__':
    unittest.main()
