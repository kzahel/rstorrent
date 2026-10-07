"""Portable safety checks for the physical qualification driver."""
import shlex
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch
import xml.etree.ElementTree as ET

import android_reactive_surface as product
from chromeos_android_qualification import observation_result, PACKAGE, RemoteAdb, confirm_intake, finish_first_use, native_maximize_arguments, reset_owned_profile, create_owned_fixture, observation_payload_size


class QualificationSafety(unittest.TestCase):
    def test_concurrent_run_fixtures_have_distinct_swarm_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first = create_owned_fixture(root / "a", "cohort-a", "observation", 256 * 1024)
            second = create_owned_fixture(root / "b", "cohort-b", "observation", 256 * 1024)
            self.assertNotEqual(first.info_hash, second.info_hash)
            self.assertEqual(first.payload_hash, second.payload_hash)
            self.assertNotEqual(first.payload_path.parent.name, second.payload_path.parent.name)

    def test_hourly_transfer_budget_has_margin_and_bounded_payload(self):
        size = observation_payload_size(3600)
        self.assertGreater(size, (3600 + 600) * 8 * 1024)
        self.assertLessEqual(size, 40 * 1024 * 1024)
        self.assertEqual(observation_payload_size(120), 28 * 1024 * 1024)

    def test_folder_tap_must_be_confirmed_by_the_owned_breadcrumb(self):
        snapshots = iter([
            '<hierarchy><node text="Select folder" clickable="true" bounds="[1,1][2,2]"/></hierarchy>',
            f'<hierarchy><node text="{product.GRANT_FOLDER}" resource-id="android:id/title" bounds="[3,3][4,4]"/>'
            '<node text="Use this folder" clickable="true" enabled="false" bounds="[5,5][6,6]"/></hierarchy>',
            f'<hierarchy><node text="{product.GRANT_FOLDER}" resource-id="android:id/title" bounds="[7,7][8,8]"/>'
            '<node text="Use this folder" clickable="true" enabled="false" bounds="[5,5][6,6]"/></hierarchy>',
            f'<hierarchy><node text="{product.GRANT_FOLDER}" resource-id="picker:id/breadcrumb_text"/>'
            '<node text="Use this folder" clickable="true" enabled="false" bounds="[5,5][6,6]"/></hierarchy>',
            f'<hierarchy><node text="{product.GRANT_FOLDER}" resource-id="picker:id/breadcrumb_text"/>'
            '<node text="Use this folder" clickable="true" enabled="true" bounds="[9,9][10,10]"/></hierarchy>',
            '<hierarchy><node package="org.rstorrent.bootstrap"/></hierarchy>',
        ])
        taps = []
        with patch.object(product, 'dump_ui', side_effect=lambda _: ET.fromstring(next(snapshots))), \
             patch.object(product, 'tap_bounds', side_effect=lambda _, bounds: taps.append(bounds)), \
             patch.object(product.time, 'sleep'):
            adb = Mock()
            adb.shell.return_value.stdout = product.GRANT_FOLDER
            product.select_controlled_tree(adb)
        self.assertEqual(taps, ['[1,1][2,2]', '[3,3][4,4]', '[7,7][8,8]', '[9,9][10,10]'])
        self.assertEqual(list(snapshots), [])

    def test_repetition_reset_restores_permission_and_never_touches_production(self):
        class Adb:
            permission = True
            cleared = []
            def shell(self, *arguments):
                if arguments[:2] == ("pm", "clear"):
                    self.cleared.append(arguments[2])
                    self.permission = False
                elif arguments[:2] == ("pm", "grant"):
                    if arguments[2:] != (PACKAGE, "android.permission.POST_NOTIFICATIONS"):
                        raise AssertionError("unexpected permission or package")
                    self.permission = True
        adb = Adb()
        for _ in range(3):
            reset_owned_profile(adb)
            self.assertTrue(adb.permission)
        self.assertEqual(adb.cleared, [PACKAGE] * 3)
        self.assertNotIn("com.jstorrent.app", adb.cleared)

    def test_incomplete_observation_cannot_pass_at_any_duration(self):
        for seconds in (600, 3600):
            result = observation_result(seconds, 3626549, False, "partial hash")
            self.assertEqual(result["status"], "fail")
            self.assertEqual(result["completion"], "fail")
            self.assertIsNone(result["sha1"])
        self.assertEqual(observation_result(600, 28*1024*1024, True, "verified")["status"], "bounded_short_run")
        self.assertEqual(observation_result(3600, 28*1024*1024, True, "verified")["status"], "pass")

    def test_native_query_accepts_stderr_and_duplicate_same_control(self):
        result = subprocess.CompletedProcess([], 0, "", '---\n2 matches\n[button] "Maximize" at (1152,17) 32x34\n[button] "Maximize" at (1152,17) 32x34\n')
        self.assertEqual(native_maximize_arguments(result), ["--nth", "1"])
        result.stderr = '---\n0 matches\n'
        self.assertIsNone(native_maximize_arguments(result))

    def test_native_query_refuses_distinct_windows(self):
        result = subprocess.CompletedProcess([], 0, '2 matches\n[button] "Maximize" at (1,17) 32x34\n[button] "Maximize" at (1152,17) 32x34\n', "")
        with self.assertRaisesRegex(RuntimeError, "ambiguous"):
            native_maximize_arguments(result)

    def test_remote_failure_retains_transport_diagnostic(self):
        failure = subprocess.CompletedProcess([], 1, "", "cat: owned UI capture does not exist")
        with patch("chromeos_android_qualification.subprocess.run", return_value=failure):
            with self.assertRaisesRegex(RuntimeError, "owned UI capture does not exist"):
                RemoteAdb([]).shell("cat", "/sdcard/rstorrent-window.xml")

    def test_clipped_picker_uses_fresh_geometry_after_recovery(self):
        snapshots = iter([
            '<hierarchy><node text="Select folder" clickable="true" bounds="[1,1][2,2]"/></hierarchy>',
            f'<hierarchy><node package="com.android.documentsui" text="{product.GRANT_FOLDER}" resource-id="picker:id/breadcrumb_text"/>'
            '<node text="Use this folder" clickable="true" bounds="[0,0][0,0]"/></hierarchy>',
            f'<hierarchy><node package="com.android.documentsui" text="{product.GRANT_FOLDER}" resource-id="picker:id/breadcrumb_text"/>'
            '<node text="Use this folder" clickable="true" bounds="[3,3][4,4]"/></hierarchy>',
            '<hierarchy><node package="org.rstorrent.bootstrap"/></hierarchy>',
        ])
        taps, recoveries = [], []
        with patch.object(product, 'dump_ui', side_effect=lambda _: ET.fromstring(next(snapshots))), \
             patch.object(product, 'tap_bounds', side_effect=lambda _, bounds: taps.append(bounds)), \
             patch.object(product.time, 'sleep'):
            adb = Mock()
            adb.shell.return_value.stdout = product.GRANT_FOLDER
            product.select_controlled_tree(adb, recover_picker=lambda: recoveries.append(True))
        self.assertEqual(taps, ['[1,1][2,2]', '[3,3][4,4]'])
        self.assertEqual(recoveries, [True])

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
