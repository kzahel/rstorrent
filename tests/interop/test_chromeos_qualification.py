"""Portable safety checks for the physical qualification driver."""
import shlex
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
import xml.etree.ElementTree as ET

from chromeos_android_qualification import RemoteAdb, confirm_intake, finish_first_use


class QualificationSafety(unittest.TestCase):
    def test_nested_shell_preserves_untrusted_uri_as_one_argument(self):
        with tempfile.TemporaryDirectory(prefix="rstorrent-253-shell-test-") as temporary:
            root = Path(temporary)
            marker = root / "unexpected-expansion"
            value = f"magnet:?xt=urn:btih:abc&x.pe=host:1 ' \" $(touch {marker})"
            fake_adb = root / "adb"
            fake_adb.write_text("#!/usr/bin/env python3\nimport subprocess,sys\nassert sys.argv[3:5]==['shell','-n']\nraise SystemExit(subprocess.call(sys.argv[5],shell=True))\n")
            fake_adb.chmod(0o700)

            def shell(_command, *, input, **options):
                source = input.replace("adb -s", shlex.quote(str(fake_adb))+" -s")
                return REAL_RUN(["/bin/sh"], input=source, **options)

            with patch("chromeos_android_qualification.subprocess.run", side_effect=shell):
                result = RemoteAdb(["machine-control"]).shell("printf", "%s", value)
            self.assertEqual(result.stdout, value)
            self.assertFalse(marker.exists())

    def test_never_clears_device_logs(self):
        with self.assertRaisesRegex(RuntimeError, "prohibited"):
            RemoteAdb([]).run("logcat", "-c")

    def test_ui_capture_is_remapped_to_owned_path(self):
        seen = []
        def run(command, **options):
            seen.append(options["input"])
            return subprocess.CompletedProcess(command, 0, "", "")
        with patch("chromeos_android_qualification.subprocess.run", side_effect=run):
            RemoteAdb([]).shell("rm", "-f", "/sdcard/rstorrent-window.xml")
        self.assertIn("/data/local/tmp/rstorrent253-ui.xml", seen[0])
        self.assertNotIn("/sdcard/rstorrent-window.xml", seen[0])

    def confirm(self, source):
        taps = []
        with patch("chromeos_android_qualification.product.dump_ui", return_value=ET.fromstring(source)), patch("chromeos_android_qualification.product.tap_bounds", side_effect=lambda _, bounds: taps.append(bounds)):
            result = confirm_intake(object())
        return result, taps

    def test_external_confirmation_does_not_finish_file_selection(self):
        result, taps = self.confirm('<hierarchy><node text="Magnet link from another app"/><node text="Add" clickable="true" enabled="true" bounds="[1,1][2,2]"/></hierarchy>')
        self.assertFalse(result)
        self.assertEqual(len(taps), 1)

    def test_pending_metadata_does_not_confirm_disabled_add(self):
        result, taps = self.confirm('<hierarchy><node text="Fetching file information…"/><node text="Add" clickable="true" enabled="false" bounds="[1,1][2,2]"/></hierarchy>')
        self.assertFalse(result)
        self.assertEqual(taps, [])

    def test_enabled_download_finishes_selection(self):
        result, taps = self.confirm('<hierarchy><node text="Download" clickable="true" enabled="true" bounds="[1,1][2,2]"/></hierarchy>')
        self.assertTrue(result)
        self.assertEqual(len(taps), 1)

    def test_first_use_refreshes_geometry_and_observes_dismissal(self):
        snapshots = iter([
            '<hierarchy><node text="Select folder" clickable="true" bounds="[7,7][8,8]"/></hierarchy>',
            '<hierarchy><node checkable="true" checked="true" bounds="[1,1][2,2]"/>'
            '<node text="Save and continue" clickable="true" bounds="[3,3][4,4]"/></hierarchy>',
            '<hierarchy><node checkable="true" checked="false"/>'
            '<node text="Save and continue" clickable="true" bounds="[5,5][6,6]"/></hierarchy>',
            '<hierarchy><node text="Select folder" clickable="true" bounds="[7,7][8,8]"/></hierarchy>',
            '<hierarchy><node text="Select folder" clickable="true" bounds="[7,7][8,8]"/></hierarchy>',
        ])
        taps = []
        with patch("chromeos_android_qualification.product.dump_ui", side_effect=lambda _: ET.fromstring(next(snapshots))), patch("chromeos_android_qualification.product.tap_bounds", side_effect=lambda _, bounds: taps.append(bounds)), patch("chromeos_android_qualification.time.sleep"):
            finish_first_use(object())
        self.assertEqual(taps, ["[1,1][2,2]", "[5,5][6,6]"])
        self.assertEqual(list(snapshots), [])


REAL_RUN = subprocess.run

if __name__ == "__main__":
    unittest.main()
