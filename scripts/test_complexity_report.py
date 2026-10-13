#!/usr/bin/env python3
"""Self-tests for complexity_report.py (plan v3.4 section 13).

Covers: CC boundary verdicts (1/10/11/15/16), nested control flow, match,
async functions, unmeasurable files (never CC 0), baseline new/removed/
renamed/moved/improved/worsened cases, analyzer version change, and
analyzer-failure handling. Standard library only.

Run: python3 scripts/test_complexity_report.py
"""

import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = Path(__file__).resolve().parent / "complexity_report.py"

sys.path.insert(0, str(SCRIPT.parent))
import complexity_report as cr  # noqa: E402


def raw_doc(name, spaces):
    return {"name": name, "start_line": 1, "end_line": 99,
            "kind": "unit", "spaces": spaces}


def fn_space(name, cc, start=1, end=10, cog=0, sloc=10, kind="function",
             extra_spaces=None):
    return {
        "name": name, "start_line": start, "end_line": end, "kind": kind,
        "spaces": extra_spaces or [],
        "metrics": {
            "cyclomatic": {"sum": float(cc), "average": float(cc),
                             "min": float(cc), "max": float(cc)},
            "cognitive": {"sum": float(cog)},
            "loc": {"sloc": float(sloc)},
        },
    }


def norm_record(path, name, cc, start=1, end=10, cog=0, sloc=10):
    return {"file": path, "function": name, "start_line": start,
            "end_line": end, "cc": cc, "cognitive": cog, "sloc": sloc,
            "band": cr.cc_band(cc), "gate": cr.cc_gate(cc)}


def run_cli(*argv):
    return subprocess.run([sys.executable, str(SCRIPT), *argv],
                          capture_output=True, text=True)


class VerdictTest(unittest.TestCase):
    def test_cc_boundaries(self):
        self.assertEqual(cr.cc_gate(1), "Normal")
        self.assertEqual(cr.cc_gate(10), "Normal")
        self.assertEqual(cr.cc_gate(11), "Review")
        self.assertEqual(cr.cc_gate(15), "Review")
        self.assertEqual(cr.cc_gate(16), "Refactor")

    def test_cc_bands(self):
        self.assertEqual(cr.cc_band(1), "Excellent")
        self.assertEqual(cr.cc_band(5), "Excellent")
        self.assertEqual(cr.cc_band(6), "Good")
        self.assertEqual(cr.cc_band(10), "Good")
        self.assertEqual(cr.cc_band(11), "Warning")
        self.assertEqual(cr.cc_band(16), "High")
        self.assertEqual(cr.cc_band(21), "Critical")


class NormalizeTest(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.raw = self.tmp / "raw"
        self.raw.mkdir()
        self.out = self.tmp / "out"

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def write_raw(self, filename, doc):
        (self.raw / filename).write_text(json.dumps(doc), encoding="utf-8")

    def test_nested_match_async_and_impl(self):
        doc = raw_doc("src/a.rs", [
            fn_space("plain", 1),
            fn_space("with_match", 5, extra_spaces=[
                fn_space("<anonymous>", 1),  # closure: excluded
            ]),
            {"name": "S", "start_line": 20, "end_line": 30, "kind": "impl",
             "spaces": [fn_space("method", 3)]},
            fn_space("async_fn", 4),
        ])
        self.write_raw("a.rs.json", doc)
        rc = run_cli("normalize", "--raw", str(self.raw), "--out", str(self.out),
                     "--tool-version", "0.0.25", "--toolchain", "t", "--commit", "c")
        self.assertEqual(rc.returncode, 0, rc.stderr)
        data = json.loads((self.out / "complexity.json").read_text())
        names = sorted(f["function"] for f in data["functions"])
        self.assertEqual(names, ["async_fn", "method", "plain", "with_match"])
        self.assertEqual(data["unmeasurable"], [])
        summary = json.loads((self.out / "complexity-summary.json").read_text())
        self.assertEqual(summary["functions_analyzed"], 4)
        self.assertEqual(summary["max_cc"], 5)

    def test_unmeasurable_is_never_cc_zero(self):
        doc = raw_doc("src/b.rs", [
            {"name": "broken_fn", "start_line": 1, "end_line": 5, "kind": "function",
             "spaces": [], "metrics": {}},  # no cyclomatic block
            fn_space("ok_fn", 2),
        ])
        self.write_raw("b.rs.json", doc)
        rc = run_cli("normalize", "--raw", str(self.raw), "--out", str(self.out),
                     "--tool-version", "0.0.25", "--toolchain", "t", "--commit", "c")
        self.assertEqual(rc.returncode, 0, rc.stderr)
        data = json.loads((self.out / "complexity.json").read_text())
        self.assertEqual([f["function"] for f in data["functions"]], ["ok_fn"])
        self.assertEqual(len(data["unmeasurable"]), 1)
        self.assertEqual(data["unmeasurable"][0]["function"], "broken_fn")
        self.assertNotIn("cc", data["unmeasurable"][0])

    def test_corrupt_raw_json_is_error_not_zero(self):
        (self.raw / "bad.json").write_text("{not json", encoding="utf-8")
        rc = run_cli("normalize", "--raw", str(self.raw), "--out", str(self.out))
        self.assertEqual(rc.returncode, 2)
        self.assertFalse((self.out / "complexity.json").exists())

    def test_empty_raw_dir_is_error(self):
        rc = run_cli("normalize", "--raw", str(self.raw), "--out", str(self.out))
        self.assertEqual(rc.returncode, 2)


class CheckTest(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.cur = self.tmp / "cur.json"
        self.base = self.tmp / "base.json"

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def write(self, funcs, tool="0.0.25", which="cur"):
        doc = {"tool": "rust-code-analysis-cli", "tool_version": tool,
               "toolchain": "t", "commit": which,
               "functions": funcs,
               "files": {f["file"]: 1 for f in funcs}}
        (self.tmp / f"{which}.json").write_text(json.dumps(doc))

    def check(self):
        return run_cli("check", "--current", str(self.cur), "--baseline", str(self.base))

    def test_new_function_cc16_fails(self):
        Path(self.cur).write_text(json.dumps({
            "tool_version": "0.0.25", "commit": "c",
            "functions": [norm_record("src/a.rs", "old", 5),
                            norm_record("src/a.rs", "new_big", 16, start=20)],
            "files": {"src/a.rs": 2}}))
        Path(self.base).write_text(json.dumps({
            "tool_version": "0.0.25", "commit": "b",
            "functions": [norm_record("src/a.rs", "old", 5)],
            "files": {"src/a.rs": 1}}))
        rc = self.check()
        self.assertEqual(rc.returncode, 1, rc.stdout)
        self.assertIn("new function with CC 16", rc.stdout)

    def test_existing_stable_cc16_passes(self):
        rec = norm_record("src/a.rs", "big", 18)
        self.write([rec])
        self.write([rec], which="base")
        rc = self.check()
        self.assertEqual(rc.returncode, 0, rc.stdout)

    def test_existing_worsened_to_cc16_fails(self):
        self.write([norm_record("src/a.rs", "grew", 17)])
        base_doc = {"tool_version": "0.0.25", "commit": "b", "functions": [
            norm_record("src/a.rs", "grew", 12)], "files": {"src/a.rs": 1}}
        self.base.write_text(json.dumps(base_doc))
        rc = self.check()
        self.assertEqual(rc.returncode, 1, rc.stdout)
        self.assertIn("worsened", rc.stdout)

    def test_cc11_warns_only(self):
        self.write([norm_record("src/a.rs", "r", 11),
                    norm_record("src/a.rs", "n", 15, start=20)])
        self.write([], which="base")
        Path(self.base).write_text(json.dumps(
            {"tool_version": "0.0.25", "commit": "b", "functions": [], "files": {}}))
        rc = self.check()
        self.assertEqual(rc.returncode, 0, rc.stdout)
        self.assertIn("WARN", rc.stdout)

    def test_removed_function_is_not_failure(self):
        self.write([])
        Path(self.cur).write_text(json.dumps(
            {"tool_version": "0.0.25", "commit": "c", "functions": [], "files": {}}))
        self.write([norm_record("src/a.rs", "gone", 20)], which="base")
        rc = self.check()
        self.assertEqual(rc.returncode, 0, rc.stdout)
        self.assertIn("removed/renamed/moved", rc.stdout)

    def test_renamed_same_cc_is_move_not_failure(self):
        self.write([norm_record("src/b.rs", "moved_fn", 18, start=3)])
        Path(self.cur).write_text(json.dumps({
            "tool_version": "0.0.25", "commit": "c",
            "functions": [norm_record("src/b.rs", "moved_fn", 18, start=3)],
            "files": {"src/b.rs": 1}}))
        self.write([norm_record("src/a.rs", "moved_fn", 18)], which="base")
        rc = self.check()
        self.assertEqual(rc.returncode, 0, rc.stdout + rc.stderr)
        self.assertIn("moved/renamed", rc.stdout)

    def test_moved_and_worsened_fails(self):
        Path(self.cur).write_text(json.dumps({
            "tool_version": "0.0.25", "commit": "c",
            "functions": [norm_record("src/b.rs", "m", 19)],
            "files": {"src/b.rs": 1}}))
        self.write([norm_record("src/a.rs", "m", 12)], which="base")
        rc = self.check()
        self.assertEqual(rc.returncode, 1, rc.stdout)

    def test_improved_function_passes(self):
        Path(self.cur).write_text(json.dumps({
            "tool_version": "0.0.25", "commit": "c",
            "functions": [norm_record("src/a.rs", "shrunk", 8)],
            "files": {"src/a.rs": 1}}))
        self.write([norm_record("src/a.rs", "shrunk", 18)], which="base")
        rc = self.check()
        self.assertEqual(rc.returncode, 0, rc.stdout)

    def test_tool_version_change_warns_only(self):
        rec = norm_record("src/a.rs", "same", 5)
        Path(self.cur).write_text(json.dumps(
            {"tool_version": "9.9.9", "commit": "c",
             "functions": [rec], "files": {"src/a.rs": 1}}))
        self.write([rec], which="base")
        rc = self.check()
        self.assertEqual(rc.returncode, 0, rc.stdout)
        self.assertIn("version changed", rc.stdout)

    def test_missing_baseline_is_infrastructure_error(self):
        self.write([norm_record("src/a.rs", "f", 2)])
        rc = run_cli("check", "--current", str(self.cur),
                     "--baseline", str(self.tmp / "nope.json"))
        self.assertEqual(rc.returncode, 2)

    def test_cognitive_and_sloc_are_warn_only(self):
        Path(self.cur).write_text(json.dumps({
            "tool_version": "0.0.25", "commit": "c",
            "functions": [norm_record("src/a.rs", "heavy", 5, cog=25, sloc=150)],
            "files": {"src/a.rs": 1}}))
        Path(self.base).write_text(json.dumps(
            {"tool_version": "0.0.25", "commit": "b", "functions": [], "files": {}}))
        rc = self.check()
        self.assertEqual(rc.returncode, 0, rc.stdout)
        self.assertIn("warn-only", rc.stdout)


class LiveToolTest(unittest.TestCase):
    """End-to-end with the real analyzer when installed (else skipped)."""

    @unittest.skipUnless(shutil.which("rust-code-analysis-cli"),
                         "rust-code-analysis-cli not installed")
    def test_live_cc_boundaries(self):
        tmp = Path(tempfile.mkdtemp())
        try:
            src = tmp / "sample.rs"
            src.write_text(
                "fn cc1() {}\n"
                "fn cc2(x: bool) -> i32 { if x { 1 } else { 2 } }\n"
                "async fn cc_async(x: bool) -> i32 { if x { 1 } else { 2 } }\n")
            raw = tmp / "raw"
            raw.mkdir()
            r = subprocess.run(
                ["rust-code-analysis-cli", "-m", "-p", str(src),
                 "-O", "json", "-o", str(raw)],
                capture_output=True, text=True)
            self.assertEqual(r.returncode, 0, r.stderr)
            functions, unmeasurable, _files, errors = cr.load_raw_metrics(raw)
            self.assertEqual(errors, [])
            self.assertEqual(unmeasurable, [])
            by_name = {f["function"]: f["cc"] for f in functions}
            self.assertEqual(by_name.get("cc1"), 1)
            self.assertEqual(by_name.get("cc2"), 2)
            self.assertEqual(by_name.get("cc_async"), 2)
        finally:
            shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    unittest.main(verbosity=2)
