import copy
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace('-', '_'), Path(__file__).with_name(name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


notices = load('prepare-android-notices')
inspector = load('inspect-android-notices')


class AndroidNoticeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.pom = self.root / 'example.pom'
        self.pom.write_text('''<project xmlns="http://maven.apache.org/POM/4.0.0">
<groupId>example</groupId><artifactId>example</artifactId><version>1.0</version>
<licenses><license><name>Apache-2.0</name><url>https://www.apache.org/licenses/LICENSE-2.0.txt</url></license></licenses>
</project>''')
        self.artifact = self.root / 'example.aar'
        nested = io.BytesIO()
        with zipfile.ZipFile(nested, 'w') as jar:
            jar.writestr('META-INF/LICENSE', 'Original copyright and license\n')
            jar.writestr('example/CopyrightKt.class', b'not a notice')
            jar.writestr('example/License.class', b'not a notice')
        with zipfile.ZipFile(self.artifact, 'w') as aar:
            aar.writestr('classes.jar', nested.getvalue())
        self.row = {'component': 'example:example:1.0', 'file': str(self.artifact),
                    'poms': [{'component': 'example:example:1.0', 'file': str(self.pom)}]}
        self.graph = {'schema': 1, 'variant': 'debug', 'artifacts': [self.row]}
        self.catalog = json.loads((notices.ROOT / 'distribution/licenses/android-sources.json').read_text())

    def test_original_nested_notice_is_preserved_without_class_files(self):
        sections, inventory = notices.maven_notices(self.graph, self.catalog)
        self.assertIn('Original copyright and license\n', '\n'.join(sections))
        self.assertNotIn('not a notice', '\n'.join(sections))
        self.assertEqual(inventory[0]['original_notices'][0]['path'], 'classes.jar!/META-INF/LICENSE')
        self.assertEqual(inventory[0]['selected_license'], 'Apache-2.0')
        self.assertNotIn(str(self.root), json.dumps(inventory))

    def test_unknown_and_missing_grants_fail(self):
        self.pom.write_text(self.pom.read_text().replace('www.apache.org', 'unknown.example'))
        with self.assertRaisesRegex(ValueError, 'unreviewed Maven grant'):
            notices.maven_notices(self.graph, self.catalog)
        self.graph['artifacts'] = []
        with self.assertRaisesRegex(ValueError, 'inventory'):
            notices.maven_notices(self.graph, self.catalog)

    def test_multiple_grants_are_not_silently_treated_as_an_or_choice(self):
        self.pom.write_text(self.pom.read_text().replace('</licenses>',
            '<license><name>GPL-3.0</name><url>https://example.test/gpl</url></license></licenses>'))
        with self.assertRaisesRegex(ValueError, 'multiple Maven grants'):
            notices.maven_notices(self.graph, self.catalog)

    def test_parent_grant_is_bound_to_declared_parent_identity(self):
        parent = self.root / 'parent.pom'
        parent.write_text(self.pom.read_text().replace('<artifactId>example</artifactId>', '<artifactId>parent</artifactId>'))
        self.pom.write_text('''<project xmlns="http://maven.apache.org/POM/4.0.0">
<parent><groupId>example</groupId><artifactId>parent</artifactId><version>1.0</version></parent>
<artifactId>example</artifactId></project>''')
        self.row['poms'].append({'component': 'example:parent:1.0', 'file': str(parent)})
        _, result = notices.maven_notices(self.graph, self.catalog)
        self.assertEqual(len(result[0]['pom_evidence']), 2)
        self.row['poms'][1]['component'] = 'example:wrong:1.0'
        with self.assertRaisesRegex(ValueError, 'parent/coordinate'):
            notices.maven_notices(self.graph, self.catalog)

    def test_duplicate_artifact_and_entity_metadata_fail(self):
        self.graph['artifacts'].append(copy.deepcopy(self.row))
        with self.assertRaisesRegex(ValueError, 'inventory'):
            notices.maven_notices(self.graph, self.catalog)
        self.graph['artifacts'].pop()
        self.pom.write_text('<!DOCTYPE project []>' + self.pom.read_text())
        with self.assertRaisesRegex(ValueError, 'DTD/entity'):
            notices.maven_notices(self.graph, self.catalog)

    def test_archive_traversal_and_expansion_bounds_fail(self):
        with zipfile.ZipFile(self.artifact, 'w') as aar:
            aar.writestr('../NOTICE', 'outside')
        with self.assertRaisesRegex(ValueError, 'unsafe path'):
            notices.artifact_notices(self.artifact)
        with zipfile.ZipFile(self.artifact, 'w') as aar:
            aar.writestr('NOTICE', 'oversized')
        with patch.object(notices, 'MAX_TEXT', 2):
            with self.assertRaisesRegex(ValueError, 'text bound'):
                notices.artifact_notices(self.artifact)

    def test_unknown_native_dependency_requires_review(self):
        with zipfile.ZipFile(self.artifact, 'w') as aar:
            aar.writestr('jni/arm64-v8a/libsurprise.so', b'\x7fELF')
        with self.assertRaisesRegex(ValueError, 'unreviewed native AAR'):
            notices.maven_notices(self.graph, self.catalog)

    def test_changed_known_native_artifact_requires_source_review(self):
        component = 'net.java.dev.jna:jna:5.17.0'
        self.row['component'] = component
        self.row['poms'][0]['component'] = component
        self.pom.write_text(self.pom.read_text().replace('<groupId>example</groupId>', '<groupId>net.java.dev.jna</groupId>')
                           .replace('<artifactId>example</artifactId>', '<artifactId>jna</artifactId>')
                           .replace('<version>1.0</version>', '<version>5.17.0</version>'))
        with zipfile.ZipFile(self.artifact, 'a') as aar:
            aar.writestr('jni/arm64-v8a/libjnidispatch.so', b'changed native code')
        with self.assertRaisesRegex(ValueError, 'native AAR checksum changed'):
            notices.maven_notices(self.graph, self.catalog)

    def make_package(self, variant='debug', aab=False):
        output = self.root / 'assets'
        with patch.object(notices.rust, 'generate_rust_notices', return_value=(['Original Rust license'], [{'name': 'example'}], 'cargo-about 0.9.2')):
            graph = {**self.graph, 'variant': variant}
            notices.generate(graph, output)
        archive = self.root / ('app.aab' if aab else 'app.apk')
        base = 'base/' if aab else ''
        with zipfile.ZipFile(archive, 'w') as package:
            for path in output.rglob('*'):
                if path.is_file():
                    package.write(path, base + 'assets/' + path.relative_to(output).as_posix())
            for abi in inspector.ABIS:
                package.writestr(base + f'lib/{abi}/librstorrent_android.so', b'\x7fELF')
        return archive

    def test_apk_and_aab_notices_validate_the_expected_variant(self):
        for aab in (False, True):
            archive = self.make_package('release', aab)
            exported = self.root / 'exported'
            result = inspector.inspect(archive, 'release', exported)
            self.assertEqual(hashlib.sha256((exported / 'THIRD_PARTY_NOTICES.txt').read_bytes()).hexdigest(), result['notices_sha256'])
            self.assertEqual(result['maven_artifacts'], 1)
            self.assertEqual(result['native_libraries'], 2)
            with self.assertRaisesRegex(ValueError, 'wrong-variant'):
                inspector.inspect(archive, 'debug')

    def test_missing_or_changed_assets_and_extra_native_files_fail(self):
        archive = self.make_package()
        with zipfile.ZipFile(archive) as source:
            files = {name: source.read(name) for name in source.namelist()}
        for kind in ('missing', 'changed', 'extra-native'):
            modified = dict(files)
            if kind == 'missing':
                modified.pop('assets/notices/THIRD_PARTY_NOTICES.txt')
            elif kind == 'changed':
                modified['assets/notices/THIRD_PARTY_NOTICES.txt'] = b'changed license'
            else:
                modified['lib/arm64-v8a/libsurprise.so'] = b'\x7fELF'
            with zipfile.ZipFile(archive, 'w') as package:
                for name, data in modified.items():
                    package.writestr(name, data)
            with self.subTest(kind=kind), self.assertRaises((KeyError, ValueError)):
                inspector.inspect(archive, 'debug')


if __name__ == '__main__':
    unittest.main()
