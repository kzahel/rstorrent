"""Architecture mismatches must fail before any installed-state mutation."""
import unittest

from legacy_desktop_platform_rehearsal import linux_legacy_pins


class LegacyArchitecturePins(unittest.TestCase):
    def test_native_arm_never_substitutes_x64_package_or_helpers(self):
        arm_package, arm_helpers = linux_legacy_pins("aarch64")
        x64_package, x64_helpers = linux_legacy_pins("x86_64")
        self.assertNotEqual(arm_package, x64_package)
        self.assertTrue(set(arm_helpers).isdisjoint(x64_helpers))
        self.assertEqual(linux_legacy_pins("arm64"), (arm_package, arm_helpers))

    def test_unsupported_architecture_is_refused(self):
        for machine in ("", "i686", "armv7l", "riscv64"):
            with self.assertRaisesRegex(ValueError, "unsupported"):
                linux_legacy_pins(machine)


if __name__ == "__main__":
    unittest.main()
