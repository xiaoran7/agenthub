#!/usr/bin/env python3
# Architecture Line Limit & Monolith Governance Linter
# Enforces AGENTS.md code health guidelines across Rust and TypeScript/TSX sources.

import os
import sys

# Baseline caps for pre-existing legacy TypeScript/TSX files (>800 lines) prior to full decoupling.
# Rust crates have 0 grandfathered monoliths. Legacy TS/TSX files must only SHRINK over time,
# never grow. When a file is refactored below 800 lines, remove it from this baseline.
LEGACY_BASELINE_CAPS = {}

SCAN_DIRECTORIES = [
    "crates/runner/src",
    "crates/runner/resources",
    "packages/contracts/src",
    "services/api/src",
    "apps/web/src",
    "apps/web/public",
    "apps/android/app/src/main/java",
]

SCAN_FILES = [
    "crates/runner/build.rs",
]

SOURCE_EXTENSIONS = (".rs", ".ts", ".tsx", ".kt", ".js", ".css")
HARD_LIMIT_NEW_FILES = 800
WARNING_THRESHOLD = 500


def is_test_file(rel_path):
    base = os.path.basename(rel_path)
    if (
        base in ("tests.rs", "test.rs")
        or base.endswith("_test.rs")
        or base.endswith("_tests.rs")
        or base.endswith(".test.ts")
        or base.endswith(".test.tsx")
    ):
        return True
    parts = rel_path.replace("\\", "/").split("/")
    if "tests" in parts or "test" in parts:
        return True
    return False


def has_architecture_exemption(file_path):
    try:
        with open(file_path, "r", encoding="utf-8", errors="ignore") as f:
            header = "".join([f.readline() for _ in range(15)])
            return "ARCHITECTURE EXEMPTION:" in header
    except Exception:
        return False


def count_lines(file_path):
    with open(file_path, "r", encoding="utf-8", errors="ignore") as f:
        return len(f.readlines())



def collect_candidate_files(root):
    seen, out = set(), []
    for d in SCAN_DIRECTORIES:
        ad = os.path.join(root, d)
        if not os.path.exists(ad):
            continue
        for dp, _, fns in os.walk(ad):
            for fn in sorted(fns):
                if not fn.endswith(SOURCE_EXTENSIONS):
                    continue
                ap = os.path.join(dp, fn)
                rp = os.path.relpath(ap, root).replace("\\", "/")
                if rp not in seen:
                    seen.add(rp)
                    out.append((rp, ap))
    for rf in SCAN_FILES:
        ap = os.path.join(root, rf)
        if os.path.isfile(ap):
            rp = os.path.relpath(ap, root).replace("\\", "/")
            if rp not in seen:
                seen.add(rp)
                out.append((rp, ap))
    return out


def main():
    root = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
    errors, warnings = [], []
    scanned_count = skipped_test_count = clean_count = warning_count = legacy_count = exempt_count = 0

    print("=================================================================")
    print("      AgentHub Architecture Line Limits & Anti-Bloat Check       ")
    print("=================================================================\n")

    for rel_path, abs_path in collect_candidate_files(root):
        if is_test_file(rel_path):
            skipped_test_count += 1
            continue
        n = count_lines(abs_path)
        scanned_count += 1
        if has_architecture_exemption(abs_path):
            exempt_count += 1
            print(f"  [EXEMPT]   {rel_path:45s} ({n:4d} lines) - explicit architectural exemption")
            continue
        if rel_path in LEGACY_BASELINE_CAPS:
            cap = LEGACY_BASELINE_CAPS[rel_path]
            if n > cap:
                errors.append(f"BLOAT REGRESSION: Legacy file {rel_path} grew to {n} lines (cap: {cap}). Monolithic files must only shrink, not expand!")
            elif n > HARD_LIMIT_NEW_FILES:
                legacy_count += 1
                warnings.append(f"LEGACY MONOLITH: {rel_path:45s} ({n:4d} lines, cap: {cap}) - scheduled for decoupling")
            else:
                clean_count += 1
            continue
        if n > HARD_LIMIT_NEW_FILES:
            errors.append(f"LINE LIMIT EXCEEDED: {rel_path} has {n} lines (max allowed: {HARD_LIMIT_NEW_FILES}). Decompose into single-responsibility submodules!")
        elif n > WARNING_THRESHOLD:
            warning_count += 1
            warnings.append(f"REFACTOR WARNING: {rel_path:45s} ({n:4d} lines) - approaching 800 line ceiling")
        else:
            clean_count += 1

    print(f"Scanned {scanned_count} production source files (.rs/.ts/.tsx/.kt/.js/.css) (skipped {skipped_test_count} test files):")
    print(f"  - Compliant (<= 500 lines):   {clean_count}")
    print(f"  - Approaching limit (501~800): {warning_count}")
    print(f"  - Grandfathered legacy (>800): {legacy_count}")
    print(f"  - Architecture exempt:        {exempt_count}")
    print(f"  - Hard violations:            {len(errors)}\n")
    if warnings:
        print("--- Active Warnings & Monolith Decoupling Backlog ---")
        for w in warnings:
            print(f"  {w}")
        print()
    if errors:
        print("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!")
        print("  FAIL: Architecture governance check failed with violations:")
        print("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!")
        for err in errors:
            print(f"  * {err}")
        print()
        sys.exit(1)
    print("PASS: All files adhere to architectural line limits and anti-bloat ratchets.\n")
    sys.exit(0)


if __name__ == "__main__":
    main()
