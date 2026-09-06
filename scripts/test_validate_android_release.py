import importlib.util
from pathlib import Path
import struct
import unittest

spec = importlib.util.spec_from_file_location('validate_android', Path(__file__).with_name('validate-android-release.py'))
validator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(validator)


class ArtifactTests(unittest.TestCase):
    manifest = '''<manifest xmlns:android="http://schemas.android.com/apk/res/android"
        package="com.jstorrent.rstorrent" android:versionName="0.1.0" android:versionCode="1">
        <uses-sdk android:minSdkVersion="28" android:targetSdkVersion="36"/>
        <application android:debuggable="false"/></manifest>'''

    def test_rejects_bootstrap_debug_or_diagnostic_release(self):
        validator.check_manifest(self.manifest, '0.1.0', 1)
        for text in (self.manifest.replace('com.jstorrent.rstorrent', 'org.rstorrent.bootstrap'),
                     self.manifest.replace('debuggable="false"', 'debuggable="true"'),
                     self.manifest.replace('<application android:debuggable="false"/>',
                         '<application><receiver android:name="org.rstorrent.bootstrap.CommandReceiver"/></application>'),
                     self.manifest.replace('versionCode="1"', 'versionCode="2"')):
            with self.assertRaises(AssertionError):
                validator.check_manifest(text, '0.1.0', 1)

    def test_native_alignment(self):
        elf = bytearray(120)
        elf[:6] = b'\x7fELF\x02\x01'
        struct.pack_into('<Q', elf, 32, 64)
        struct.pack_into('<HH', elf, 54, 56, 1)
        struct.pack_into('<I', elf, 64, 1)
        for alignment in (16384, 65536):
            struct.pack_into('<Q', elf, 112, alignment)
            validator.check_elf(elf, 'fixture')
        for alignment in (0, 4096, 20000):
            struct.pack_into('<Q', elf, 112, alignment)
            with self.assertRaises(AssertionError):
                validator.check_elf(elf, 'fixture')


if __name__ == '__main__':
    unittest.main()
