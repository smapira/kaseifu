#!/usr/bin/env python3
"""kaseifu complexity report normalizer and quality gate.

Reads rust-code-analysis-cli JSON output (one file per analyzed source file)
and produces kaseifu-normalized reports:

- complexity.json          per-function records {file, function, start_line,
                             end_line, cc, cognitive, sloc}
- complexity-summary.json  totals, buckets, max/avg CC, KPI ratios, top-10
- ranking (stdout)        human-readable CC ranking
- baseline.json            reviewed snapshot for regression comparison
- check (stdout + exit)   threshold + baseline comparison gate

Quality bands (project-local policy, NOT official Rust thresholds):
  CC   1-10 Normal  | 11-15 Review (warn) | 16+ Refactor (fail if new/worsened)
  Cognitive 16-20 warn, 21+ warn (fail gate reserved: future candidate)
  sloc 51-100 warn, 101+ warn (fail gate reserved: staged rollout)

Functions that cannot be measured are NEVER recorded as CC 0; they are
listed under "unmeasurable" and surfaced as warnings.

Standard library only.
"""

import argparse
import datetime
import json
import subprocess
import sys
from pathlib import Path

TOOL_NAME = "rust-code-analysis-cli"
PINNED_TOOL_VERSION = "0.0.25"

CC_NORMAL_MAX = 10
CC_REVIEW_MAX = 15  # 11-15 warn, 16+ refactor candidate
COG_WARN_MIN = 16
COG_HIGH_MIN = 21
SLOC_WARN_MIN = 51
SLOC_HIGH_MIN = 101
RANKING_LIMIT = 20


def eprint(msg):
    print(msg, file=sys.stderr)


def run_capture(argv):
    try:
        out = subprocess.run(argv, capture_output=True, text=True, timeout=60)
        return out.stdout.strip()
    except Exception:
        return ""


def detect_tool_version():
    out = run_capture([TOOL_NAME, "--version"])
    # expected: "rust-code-analysis-cli 0.0.25"
    parts = out.split()
    return parts[-1] if parts else "unknown"


def detect_toolchain():
    out = run_capture(["rustc", "--version"])
    return out or "unknown"


def detect_commit():
    out = run_capture(["git", "rev-parse", "HEAD"])
    return out or "unknown"


def cc_band(cc):
    if cc <= 5:
        return "Excellent"
    if cc <= 10:
        return "Good"
    if cc <= 15:
        return "Warning"
    if cc <= 20:
        return "High"
    return "Critical"


def cc_gate(cc):
    if cc <= CC_NORMAL_MAX:
        return "Normal"
    if cc <= CC_REVIEW_MAX:
        return "Review"
    return "Refactor"


def to_int_metric(value):
    """Convert a metric number to int. Returns None when not measurable."""
    if isinstance(value, bool):
        return None
    if isinstance(value, (int, float)):
        return int(value)
    return None


def walk_spaces(space, file_label, functions, unmeasurable):
    for child in space.get("spaces", []):
        kind = child.get("kind")
        name = child.get("name")
        if kind == "function" and name and name != "<anonymous>":
            metrics = child.get("metrics", {}) or {}
            cc = to_int_metric((metrics.get("cyclomatic") or {}).get("max"))
            cog = to_int_metric((metrics.get("cognitive") or {}).get("sum"))
            sloc = to_int_metric((metrics.get("loc") or {}).get("sloc"))
            record = {
                "file": file_label,
                "function": name,
                "start_line": child.get("start_line"),
                "end_line": child.get("end_line"),
            }
            if cc is None:
                record["reason"] = "cyclomatic metric missing"
                unmeasurable.append(record)
            else:
                record["cc"] = cc
                record["cognitive"] = cog
                record["sloc"] = sloc
                record["band"] = cc_band(cc)
                record["gate"] = cc_gate(cc)
                functions.append(record)
        # Recurse into every subspace (impl blocks, nested items, closures).
        walk_spaces(child, file_label, functions, unmeasurable)


def load_raw_metrics(raw_dir):
    """Returns (functions, unmeasurable, files, errors)."""
    raw = Path(raw_dir)
    json_files = sorted(raw.rglob("*.json"))
    functions, unmeasurable, errors = [], [], []
    files = {}
    for path in json_files:
        try:
            doc = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            errors.append({"raw_file": str(path), "reason": f"unreadable: {exc}"})
            continue
        label = doc.get("name") or str(path)
        label = str(label).replace("\\", "/").lstrip("./")
        before = len(functions) + len(unmeasurable)
        walk_spaces(doc, label, functions, unmeasurable)
        files[label] = len(functions) + len(unmeasurable) - before
    return functions, unmeasurable, files, errors


def summarize(functions, unmeasurable, files, meta):
    ccs = [f["cc"] for f in functions]
    total = len(functions)
    normal = sum(1 for c in ccs if c <= CC_NORMAL_MAX)
    review = sum(1 for c in ccs if CC_NORMAL_MAX < c <= CC_REVIEW_MAX)
    refactor = sum(1 for c in ccs if c > CC_REVIEW_MAX)
    cog_high = sum(1 for f in functions
                   if f.get("cognitive") is not None and f["cognitive"] >= COG_HIGH_MIN)
    sloc_high = sum(1 for f in functions
                    if f.get("sloc") is not None and f["sloc"] >= SLOC_HIGH_MIN)
    ranked = sorted(functions, key=lambda f: (-f["cc"], f["file"], f["function"]))
    summary = {
        **meta,
        "functions_analyzed": total,
        "files_analyzed": len(files),
        "cc_1_10": normal,
        "cc_11_15": review,
        "cc_16_plus": refactor,
        "cc_10_or_below_ratio": (normal / total) if total else 1.0,
        "max_cc": max(ccs) if ccs else 0,
        "avg_cc": (sum(ccs) / total) if total else 0.0,
        "cognitive_21_plus": cog_high,
        "sloc_101_plus": sloc_high,
        "unmeasurable_functions": len(unmeasurable),
        "top10": [
            {k: f[k] for k in ("file", "function", "start_line", "end_line",
                               "cc", "cognitive", "sloc", "band", "gate")}
            for f in ranked[:10]
        ],
    }
    return summary, ranked


def print_ranking(ranked, summary):
    print("KASEIFU / CODE QUALITY")
    print("")
    print("Cyclomatic Complexity")
    print("")
    print(f"{'Rank':<6}{'CC':<6}{'Cog':<6}{'SLOC':<7}Function                 File")
    print("-" * 72)
    for i, f in enumerate(ranked[:RANKING_LIMIT], start=1):
        print(f"{i:<6}{f['cc']:<6}{f.get('cognitive', '?')!s:<6}"
              f"{f.get('sloc', '?')!s:<7}{f['function']:<25}{f['file']}:{f['start_line']}")
    if len(ranked) > RANKING_LIMIT:
        print(f"... and {len(ranked) - RANKING_LIMIT} more functions")
    print("")
    print("Summary")
    print("-" * 72)
    print(f"Functions analyzed : {summary['functions_analyzed']}")
    print(f"CC 1-10            : {summary['cc_1_10']}")
    print(f"CC 11-15           : {summary['cc_11_15']}")
    print(f"CC 16+             : {summary['cc_16_plus']}")
    print(f"Maximum CC         : {summary['max_cc']}")
    print(f"Average CC         : {summary['avg_cc']:.2f}")
    print(f"Unmeasurable       : {summary['unmeasurable_functions']} (never counted as CC 0)")


def cmd_normalize(args):
    tool_version = args.tool_version or detect_tool_version()
    functions, unmeasurable, files, errors = load_raw_metrics(args.raw)
    if errors:
        for err in errors:
            eprint(f"ERROR: raw report {err['raw_file']}: {err['reason']}")
        eprint("Refusing to emit a normalized report from incomplete raw data "
               "(missing functions must not be treated as CC 0).")
        return 2
    if not functions and not files:
        eprint(f"ERROR: no rust-code-analysis JSON reports found under {args.raw}.")
        return 2
    meta = {
        "tool": TOOL_NAME,
        "tool_version": tool_version,
        "toolchain": args.toolchain or detect_toolchain(),
        "commit": args.commit or detect_commit(),
        "generated_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    }
    summary, ranked = summarize(functions, unmeasurable, files, meta)
    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "complexity.json").write_text(
        json.dumps({**meta, "functions": functions,
                    "unmeasurable": unmeasurable, "files": files}, indent=2) + "\n",
        encoding="utf-8")
    (out_dir / "complexity-summary.json").write_text(
        json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print_ranking(ranked, summary)
    if unmeasurable:
        eprint(f"WARNING: {len(unmeasurable)} function(s) could not be measured; "
               "excluded from statistics (not CC 0).")
        for u in unmeasurable:
            eprint(f"  - {u['file']}:{u.get('start_line')} {u.get('function')}")
    return 0


def cmd_baseline(args):
    current = json.loads(Path(args.current).read_text(encoding="utf-8"))
    baseline = {
        "tool": current.get("tool", TOOL_NAME),
        "tool_version": current.get("tool_version", "unknown"),
        "toolchain": current.get("toolchain", "unknown"),
        "commit": args.commit or current.get("commit", "unknown"),
        "generated_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "thresholds": {
            "cc_normal_max": CC_NORMAL_MAX,
            "cc_review_max": CC_REVIEW_MAX,
            "cognitive_warn": COG_WARN_MIN,
            "cognitive_high": COG_HIGH_MIN,
            "sloc_warn": SLOC_WARN_MIN,
            "sloc_high": SLOC_HIGH_MIN,
        },
        "functions": current.get("functions", []),
        "files": current.get("files", {}),
    }
    Path(args.out).write_text(json.dumps(baseline, indent=2) + "\n", encoding="utf-8")
    print(f"Baseline written to {args.out} "
          f"({len(baseline['functions'])} functions, "
          f"tool {baseline['tool_version']}, commit {baseline['commit']}).")
    return 0


def index_by_key(functions):
    index = {}
    for f in functions:
        index.setdefault((f["file"], f["function"]), []).append(f)
    for key in index:
        index[key].sort(key=lambda f: -f["cc"])
    return index


def cmd_check(args):
    try:
        current = json.loads(Path(args.current).read_text(encoding="utf-8"))
        baseline = json.loads(Path(args.baseline).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        eprint(f"ERROR: cannot load report for baseline comparison: {exc}")
        return 2
    cur = index_by_key(current.get("functions", []))
    base = index_by_key(baseline.get("functions", []))
    base_tool = baseline.get("tool_version", "unknown")
    cur_tool = current.get("tool_version", "unknown")

    failures, warnings, notes = [], [], []
    if base_tool != cur_tool:
        warnings.append(f"analyzer version changed: baseline={base_tool} current={cur_tool} "
                        "(treat small CC shifts with suspicion)")

    # Baseline entries with no current counterpart are "freed" (removed / renamed /
    # moved). A new key matching a freed name with equal-or-lower CC is a move,
    # never a worsening.
    freed_by_name = {}
    for (bfile, bfn), entries in base.items():
        if (bfile, bfn) not in cur:
            for e in entries:
                freed_by_name.setdefault(bfn, []).append({"file": bfile, "cc": e["cc"]})

    for key, entries in sorted(cur.items()):
        cfile, cfn = key
        base_entries = base.get(key, [])
        for i, entry in enumerate(entries):
            cc = entry["cc"]
            where = f"{cfile}:{entry.get('start_line')} {cfn} (CC {cc})"
            if i < len(base_entries):
                prev = base_entries[i]["cc"]
                if cc > CC_REVIEW_MAX and cc > prev:
                    failures.append(f"worsened to CC {cc} (baseline {prev}): {where}")
                elif CC_NORMAL_MAX < cc <= CC_REVIEW_MAX:
                    warnings.append(f"CC {cc} needs review: {where}")
                elif cc > prev:
                    notes.append(f"CC {prev} -> {cc} (within Normal): {where}")
            else:
                moved = [m for m in freed_by_name.get(cfn, [])]
                same_cc = [m for m in moved if m["cc"] == cc]
                lower_cc = [m for m in moved if m["cc"] < cc]
                if same_cc:
                    notes.append(f"moved/renamed (CC unchanged at {cc}): {where} "
                                 f"<- {same_cc[0]['file']}")
                elif lower_cc and cc > CC_REVIEW_MAX:
                    best = max(m["cc"] for m in lower_cc)
                    failures.append(f"moved/renamed and worsened to CC {cc} "
                                    f"(was {best}): {where}")
                elif lower_cc:
                    notes.append(f"moved/renamed (CC {max(m['cc'] for m in lower_cc)} -> {cc}): {where}")
                elif moved:
                    notes.append(f"moved/renamed with improved CC ({cc}): {where}")
                elif cc > CC_REVIEW_MAX:
                    failures.append(f"new function with CC {cc} (>= 16): {where}")
                elif cc > CC_NORMAL_MAX:
                    warnings.append(f"new function with CC {cc} needs review: {where}")
            if (entry.get("cognitive") is not None
                    and entry["cognitive"] >= COG_HIGH_MIN):
                warnings.append(f"cognitive {entry['cognitive']} >= {COG_HIGH_MIN} "
                                f"(warn-only gate): {where}")
            if entry.get("sloc") is not None and entry["sloc"] >= SLOC_HIGH_MIN:
                warnings.append(f"{entry['sloc']} lines >= {SLOC_HIGH_MIN} "
                                f"(warn-only gate): {where}")

    cur_files = set(current.get("files", {}))
    for (bfile, _bfn) in sorted(base):
        if bfile not in cur_files:
            notes.append(f"file no longer analyzed (removed or unparsable?): {bfile}")
        elif current.get("files", {}).get(bfile) == 0:
            warnings.append(f"file analyzed with ZERO functions (parse degradation?): {bfile}")
    for bkey in sorted(base):
        if bkey not in cur:
            notes.append(f"function removed/renamed/moved: {bkey[0]}::{bkey[1]} (not a failure)")

    print("BASELINE COMPARISON")
    print(f"baseline: tool {base_tool}, commit {baseline.get('commit', '?')}")
    print(f"current : tool {cur_tool}, commit {current.get('commit', '?')}")
    for w in warnings:
        print(f"WARN: {w}")
    for n in notes:
        print(f"note: {n}")
    if failures:
        for f in failures:
            print(f"FAIL: {f}")
        print(f"RESULT: FAIL ({len(failures)} blocking, {len(warnings)} warnings)")
        return 1
    print(f"RESULT: PASS ({len(warnings)} warnings)")
    return 0


def build_parser():
    p = argparse.ArgumentParser(description="kaseifu complexity report tool")
    sub = p.add_subparsers(dest="command", required=True)
    n = sub.add_parser("normalize", help="normalize raw analyzer JSON")
    n.add_argument("--raw", required=True)
    n.add_argument("--out", required=True)
    n.add_argument("--tool-version", default=None)
    n.add_argument("--toolchain", default=None)
    n.add_argument("--commit", default=None)
    b = sub.add_parser("baseline", help="snapshot current report as baseline")
    b.add_argument("--current", required=True)
    b.add_argument("--out", required=True)
    b.add_argument("--commit", default=None)
    c = sub.add_parser("check", help="threshold + baseline gate")
    c.add_argument("--current", required=True)
    c.add_argument("--baseline", required=True)
    return p


def main(argv=None):
    args = build_parser().parse_args(argv)
    if args.command == "normalize":
        return cmd_normalize(args)
    if args.command == "baseline":
        return cmd_baseline(args)
    return cmd_check(args)


if __name__ == "__main__":
    sys.exit(main())
