#!/usr/bin/env python3
# Architecture Line Limit & Monolith Governance Linter
# Enforces AGENTS.md code health guidelines across Rust and TypeScript/TSX sources.

import os
import sys

# Baseline caps for pre-existing legacy TypeScript/TSX files (>800 lines) prior to full decoupling.
# Rust crates have 0 grandfathered monoliths. Legacy TS/TSX files must only SHRINK over time,
# never grow. When a file is refactored below 800 lines, remove it from this baseline.
LEGACY_BASELINE_CAPS = {
    "apps/web/src/components/RemoteConsole.tsx": 5370,
    "apps/web/src/components/SettingsPage.tsx": 1365,
    "packages/contracts/src/index.ts": 1152,
    "services/api/src/accounts.ts": 1960,
    "services/api/src/app.ts": 1861,
    "services/api/src/postgres-store.ts": 3067,
    "services/api/src/remote-store.ts": 975,
    "services/api/src/store.ts": 822,
}

SCAN_DIRECTORIES = [
    "crates/runner/src",
    "packages/contracts/src",
    "services/api/src",
    "apps/web/src",
    "apps/android/app/src/main/java",
]

SOURCE_EXTENSIONS = (".rs", ".ts", ".tsx", ".kt")
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


def main():
    root = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
    errors = []
    warnings = []
    scanned_count = 0
    clean_count = 0
    warning_count = 0
    legacy_count = 0

    print("=================================================================")
    print("      AgentHub Architecture Line Limits & Anti-Bloat Check       ")
    print("=================================================================\n")

    for scan_dir in SCAN_DIRECTORIES:
        abs_scan_dir = os.path.join(root, scan_dir)
        if not os.path.exists(abs_scan_dir):
            continue

        for dirpath, _, filenames in os.walk(abs_scan_dir):
            for filename in sorted(filenames):
                if not filename.endswith(SOURCE_EXTENSIONS):
                    continue

                abs_path = os.path.join(dirpath, filename)
                rel_path = os.path.relpath(abs_path, root).replace("\\", "/")
                lines = count_lines(abs_path)
                scanned_count += 1

                if is_test_file(rel_path):
                    continue

                if has_architecture_exemption(abs_path):
                    print(
                        f"  [EXEMPT]   {rel_path:45s} ({lines:4d} lines) - explicit architectural exemption"
                    )
                    continue

                if rel_path in LEGACY_BASELINE_CAPS:
                    cap = LEGACY_BASELINE_CAPS[rel_path]
                    if lines > cap:
                        errors.append(
                            f"BLOAT REGRESSION: Legacy file {rel_path} grew to {lines} lines (cap: {cap}). "
                            f"Monolithic files must only shrink, not expand!"
                        )
                    elif lines > HARD_LIMIT_NEW_FILES:
                        legacy_count += 1
                        warnings.append(
                            f"LEGACY MONOLITH: {rel_path:45s} ({lines:4d} lines, cap: {cap}) - scheduled for decoupling"
                        )
                    else:
                        clean_count += 1
                    continue

                if lines > HARD_LIMIT_NEW_FILES:
                    errors.append(
                        f"LINE LIMIT EXCEEDED: {rel_path} has {lines} lines (max allowed: {HARD_LIMIT_NEW_FILES}). "
                        f"Decompose into single-responsibility submodules!"
                    )
                elif lines > WARNING_THRESHOLD:
                    warning_count += 1
                    warnings.append(
                        f"REFACTOR WARNING: {rel_path:45s} ({lines:4d} lines) - approaching 800 line ceiling"
                    )
                else:
                    clean_count += 1

    print(f"Scanned {scanned_count} source files (.rs/.ts/.tsx):")
    print(f"  - Compliant (<= 500 lines):   {clean_count}")
    print(f"  - Approaching limit (501~800): {warning_count}")
    print(f"  - Grandfathered legacy (>800): {legacy_count}")
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
