#!/usr/bin/env python3

from __future__ import annotations

import os
import subprocess
import sys
import unittest

from wan_transport_resource_sampler import process_sample, sample


class WanTransportResourceSamplerTests(unittest.TestCase):
    def test_current_process_sample_is_bounded(self) -> None:
        result = process_sample(os.getpid())
        self.assertIsNotNone(result)
        assert result is not None
        self.assertGreater(result[0], 0)
        self.assertGreaterEqual(result[1], 0)

    def test_short_child_is_sampled_and_joined(self) -> None:
        child = subprocess.Popen(
            [sys.executable, "-c", "import time; time.sleep(0.35)"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        try:
            result = sample(child.pid, 0.25, 2)
        finally:
            child.wait(timeout=2)
        self.assertGreaterEqual(result["samples"], 1)
        self.assertGreater(result["process"]["rss_high_water_kib"], 0)


if __name__ == "__main__":
    unittest.main()
