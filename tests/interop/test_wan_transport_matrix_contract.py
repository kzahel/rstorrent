#!/usr/bin/env python3

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from wan_transport_matrix_contract import (
    GIB,
    MIB,
    PIECE_BYTES,
    Journal,
    MatrixContractError,
    assert_redacted,
    manifest,
    pending_cases,
    select_cases,
    summarize,
    validate_ssh_alias,
)


def complete_record(case: dict[str, object], seconds: float = 2.0) -> dict[str, object]:
    return {
        "schema_version": 1,
        "event": "case-terminal",
        "case": case,
        "status": "complete",
        "result": {
            "timing": {
                "connect_to_complete_seconds": seconds + 1.0,
                "active_payload_seconds": seconds,
            },
            "integrity": {"verified": True},
        },
        "cleanup": {"succeeded": True},
    }


class ManifestTests(unittest.TestCase):
    def test_one_repetition_has_every_exact_cell(self) -> None:
        cases = manifest("baseline")
        self.assertEqual(len(cases), 64)
        self.assertEqual(len({case.case_id for case in cases}), 64)
        self.assertEqual(
            {
                (
                    case.size_mib,
                    case.direction,
                    case.seed,
                    case.leech,
                    case.transport,
                )
                for case in cases
            },
            {
                (size, direction, seed, leech, transport)
                for size in (8, 64, 256, 1024)
                for direction in ("local-seed", "remote-seed")
                for seed in ("rstorrent", "libtorrent")
                for leech in ("rstorrent", "libtorrent")
                for transport in ("tcp", "utp")
            },
        )
        largest = next(case for case in cases if case.size_mib == 1024)
        self.assertEqual(largest.payload_bytes, GIB)
        self.assertEqual(largest.piece_count, GIB // PIECE_BYTES)
        self.assertEqual(largest.timeout_seconds, 12 * 60 * 60)

    def test_repetitions_rotate_order_without_changing_cells(self) -> None:
        cases = manifest("repeated", repetitions=3)
        self.assertEqual(len(cases), 192)
        first = [case for case in cases if case.repetition == 1]
        second = [case for case in cases if case.repetition == 2]
        self.assertNotEqual(
            [(case.size_mib, case.direction, case.transport) for case in first],
            [(case.size_mib, case.direction, case.transport) for case in second],
        )

    def test_selection_is_closed_and_case_addressable(self) -> None:
        cases = manifest("baseline")
        selected = select_cases(
            cases,
            sizes_mib=[8],
            directions=["remote-seed"],
            seeds=["libtorrent"],
            leeches=["rstorrent"],
            transports=["utp"],
        )
        self.assertEqual(len(selected), 1)
        self.assertEqual(selected[0].payload_bytes, 8 * MIB)
        self.assertEqual(
            select_cases(cases, case_ids=[selected[0].case_id]), selected
        )
        with self.assertRaises(MatrixContractError):
            select_cases(cases, case_ids=["missing-case"])


class JournalTests(unittest.TestCase):
    def test_append_resume_and_truncated_tail_repair(self) -> None:
        cases = manifest("baseline")[:2]
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "results.jsonl"
            journal = Journal(path)
            first = complete_record(cases[0].public_dict())
            journal.append(first)
            with path.open("ab") as output:
                output.write(b'{"schema_version":1')
            self.assertEqual(journal.load(repair_truncated_tail=True), [first])
            journal.append(complete_record(cases[1].public_dict(), seconds=4.0))
            self.assertEqual(journal.completed_case_ids(), {case.case_id for case in cases})
            self.assertEqual(pending_cases(cases, journal), [])

    def test_duplicate_and_unclean_records_are_rejected(self) -> None:
        case = manifest("baseline")[0].public_dict()
        with tempfile.TemporaryDirectory() as temporary:
            journal = Journal(Path(temporary) / "results.jsonl")
            record = complete_record(case)
            journal.append(record)
            with self.assertRaises(MatrixContractError):
                journal.append(record)
            unclean = complete_record(manifest("other")[0].public_dict())
            unclean["cleanup"] = {"succeeded": False}
            with self.assertRaises(MatrixContractError):
                journal.append(unclean)


class SummaryAndPrivacyTests(unittest.TestCase):
    def test_summary_never_calls_one_sample_stable(self) -> None:
        case = manifest("baseline")[0].public_dict()
        report = summarize([complete_record(case, seconds=2.0)])
        self.assertEqual(report["complete"], 1)
        self.assertFalse(report["cells"][0]["stable"])
        self.assertEqual(
            report["cells"][0]["active_mib_per_second"]["median"],
            case["payload_bytes"] / MIB / 2.0,
        )

    def test_privacy_rejects_addresses_paths_urls_and_aliases(self) -> None:
        assert_redacted(
            {"libtorrent_version": "2.0.13.0", "route_class": "ordinary-internet"}
        )
        for value in (
            "198.51.100.4",
            "2001:db8::1",
            "/tmp/private",
            "https://gateway.invalid/control",
            "case for secret-host",
        ):
            with self.subTest(value=value), self.assertRaises(MatrixContractError):
                assert_redacted({"value": value}, forbidden_strings=["secret-host"])

    def test_ssh_alias_is_bounded(self) -> None:
        self.assertEqual(validate_ssh_alias("pimom"), "pimom")
        for value in ("-host", "host name", "", "x" * 129):
            with self.subTest(value=value), self.assertRaises(MatrixContractError):
                validate_ssh_alias(value)


if __name__ == "__main__":
    unittest.main()
