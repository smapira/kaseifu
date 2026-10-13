#!/usr/bin/env bash
# kaseifu quality gate - run locally and in CI with the same script.
#
# Usage: bash scripts/quality_check.sh
#
# Steps: rustfmt, Clippy (-D warnings), cargo test, complexity analysis
# (rust-code-analysis-cli + scripts/complexity_report.py), report self-test,
# best-effort coverage. Every step keeps its exit code, failures are
# aggregated, remaining checks still run, and a final summary decides
# OVERALL PASS/FAIL. Exit 0 on pass, 1 on failure.
#
# Complexity tool setup (once):
#   cargo install rust-code-analysis-cli --version 0.0.25 --locked
# NOTE: --locked is required; without it the 2023-era dependency set drifts
# (tree-sitter type mismatch, E0308) on modern toolchains.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

REPORT_DIR="reports/quality"
LOG_DIR="$REPORT_DIR/logs"
RAW_DIR="$REPORT_DIR/raw"
BASELINE="$REPORT_DIR/baseline.json"
COMPLEXITY_JSON="$REPORT_DIR/complexity.json"
SUMMARY_JSON="$REPORT_DIR/complexity-summary.json"
mkdir -p "$LOG_DIR" "$RAW_DIR"

PASS=0
FAIL=0
GATE_FAIL=0
declare -a ROWS=()

record() { # name status detail gating(y/n)
    ROWS+=("$1|$2|$3")
    if [ "$2" = "PASS" ] || [ "$2" = "SKIP" ]; then
        PASS=$((PASS + 1))
    else
        FAIL=$((FAIL + 1))
        if [ "${4:-y}" = "y" ]; then
            GATE_FAIL=1
        fi
    fi
    printf '%-22s %-6s %s\n' "$1" "$2" "$3"
}

run_step() { # logfile, then command...
    local log="$1"; shift
    "$@" >"$log" 2>&1
    return $?
}

echo "KASEIFU / QUALITY CHECK"
echo "root: $ROOT"
echo ""

# 1. rustfmt (CI fails on any violation)
if run_step "$LOG_DIR/fmt.log" cargo fmt --all -- --check; then
    record "rustfmt" "PASS" "violations 0" y
else
    record "rustfmt" "FAIL" "violations found (see $LOG_DIR/fmt.log)" y
fi

# 2. Clippy (CI fails on any warning; no broad allow in codebase)
if run_step "$LOG_DIR/clippy.log" cargo clippy --all-targets --all-features -- -D warnings; then
    record "clippy" "PASS" "warnings 0" y
else
    record "clippy" "FAIL" "warnings found (see $LOG_DIR/clippy.log)" y
fi

# 3. cargo test (real API keys never required: mock-based suite)
if run_step "$LOG_DIR/test.log" cargo test --all-targets --all-features; then
    PASSED=$(grep -h "test result:" "$LOG_DIR/test.log" | awk -F'[ ;]+' '{p+=$4} END {print p+0}')
    FAILED=$(grep -h "test result:" "$LOG_DIR/test.log" | awk -F'[ ;]+' '{f+=$6} END {print f+0}')
    record "cargo-test" "PASS" "passed $PASSED, failed $FAILED" y
else
    FAILED=$(grep -h "test result:" "$LOG_DIR/test.log" | awk -F'[ ;]+' '{f+=$6} END {print f+0}')
    record "cargo-test" "FAIL" "failures: ${FAILED:-?} (see $LOG_DIR/test.log)" y
fi

# 4. Complexity analysis
if ! command -v rust-code-analysis-cli >/dev/null 2>&1; then
    record "complexity" "FAIL" "tool missing; run: cargo install rust-code-analysis-cli --version 0.0.25 --locked" y
else
    INSTALLED_VERSION="$(rust-code-analysis-cli --version 2>/dev/null | awk '{print $NF}')"
    rm -rf "$RAW_DIR"
    mkdir -p "$RAW_DIR"
    if run_step "$LOG_DIR/complexity-analyze.log" rust-code-analysis-cli -m -p src -O json -o "$RAW_DIR"; then
        if run_step "$LOG_DIR/complexity-normalize.log" python3 scripts/complexity_report.py normalize --raw "$RAW_DIR" --out "$REPORT_DIR" --tool-version "$INSTALLED_VERSION"; then
            record "complexity" "PASS" "analyzer $INSTALLED_VERSION" y
            cat "$LOG_DIR/complexity-normalize.log"
        else
            record "complexity" "FAIL" "normalization failed; refusing to treat gaps as CC 0 (see $LOG_DIR/complexity-normalize.log)" y
        fi
    else
        record "complexity" "FAIL" "analyzer failed; not treated as CC 0 (see $LOG_DIR/complexity-analyze.log)" y
    fi
fi

# 5. Baseline comparison (existing high-CC code never fails the gate by itself;
# only new functions >= 16 or worsening to >= 16 fail)
if [ -f "$COMPLEXITY_JSON" ] && [ -f "$BASELINE" ]; then
    if run_step "$LOG_DIR/baseline-check.log" python3 scripts/complexity_report.py check --current "$COMPLEXITY_JSON" --baseline "$BASELINE"; then
        record "baseline-gate" "PASS" "no new/worsened CC>=16" y
    else
        record "baseline-gate" "FAIL" "new or worsened CC>=16 (see $LOG_DIR/baseline-check.log)" y
    fi
    cat "$LOG_DIR/baseline-check.log"
elif [ -f "$COMPLEXITY_JSON" ]; then
    record "baseline-gate" "SKIP" "no baseline yet; create: python3 scripts/complexity_report.py baseline --current $COMPLEXITY_JSON --out $BASELINE" y
else
    record "baseline-gate" "SKIP" "no complexity report to compare" y
fi

# 6. Report self-test (section 13 cases)
if run_step "$LOG_DIR/report-selftest.log" python3 scripts/test_complexity_report.py; then
    record "report-tests" "PASS" "complexity_report self-tests green" y
else
    record "report-tests" "FAIL" "self-test failure (see $LOG_DIR/report-selftest.log)" y
fi

# 7. Coverage: report-only, never gates (cargo-llvm-cov staged rollout per plan).
if command -v cargo-llvm-cov >/dev/null 2>&1; then
    if [ -z "${LLVM_COV:-}" ]; then
        # Non-rustup toolchains ship no llvm-tools; fall back to Xcode's.
        XCCOV="$(xcrun -f llvm-cov 2>/dev/null || true)"
        XPROF="$(xcrun -f llvm-profdata 2>/dev/null || true)"
        [ -n "$XCCOV" ] && export LLVM_COV="$XCCOV"
        [ -n "$XPROF" ] && export LLVM_PROFDATA="$XPROF"
    fi
    if run_step "$LOG_DIR/coverage.log" cargo llvm-cov --all-features --summary-only; then
        COV=$(grep -a "^TOTAL" "$LOG_DIR/coverage.log" | grep -oE "[0-9]+\.[0-9]+%" | sed -n '3p')
        record "coverage" "PASS" "report-only (line ${COV:-see log})" n
    else
        record "coverage" "FAIL" "report-only, tool failed (see $LOG_DIR/coverage.log)" n
    fi
else
    record "coverage" "SKIP" "report-only (cargo-llvm-cov not installed)" n
fi

echo ""
echo "KASEIFU / QUALITY REPORT"
echo ""
if [ -f "$SUMMARY_JSON" ]; then
    python3 - "$SUMMARY_JSON" <<'EOF'
import json, sys
s = json.load(open(sys.argv[1]))
total = s["functions_analyzed"]
ratio = s["cc_10_or_below_ratio"] * 100
print("Cyclomatic Complexity")
print(f"  Functions analyzed     {total}")
print(f"  CC <= 10               {s['cc_1_10']}          {ratio:.1f}%")
print(f"  CC 11-15               {s['cc_11_15']}")
print(f"  CC >= 16               {s['cc_16_plus']}")
print(f"  Maximum CC             {s['max_cc']}")
print(f"  Average CC             {s['avg_cc']:.2f}")
print(f"  Target <= 10           95%         {'PASS' if ratio >= 95 else 'WATCH'}")
print(f"  Cognitive >= 21        {s['cognitive_21_plus']} (warn-only)")
print(f"  Lines >= 101           {s['sloc_101_plus']} (warn-only)")
print(f"  Unmeasurable           {s['unmeasurable_functions']} (excluded, never CC 0)")
EOF
fi
echo ""
for row in "${ROWS[@]}"; do
    echo "  $row"
done
echo ""
if [ "$GATE_FAIL" -eq 0 ]; then
    echo "OVERALL: PASS"
    exit 0
else
    echo "OVERALL: FAIL"
    exit 1
fi
