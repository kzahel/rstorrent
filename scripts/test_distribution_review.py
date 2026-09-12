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


if __name__ == '__main__':
    unittest.main()
