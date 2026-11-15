#!/usr/bin/env python3
"""Black-box tests for state-check. Run: python3 -m unittest discover -s scripts/tests

Each test builds a throwaway project (spec + STATE.md + ledger + fake tests and
entry point), runs the real script as a subprocess, and asserts on its exit code
and message. No test imports the checker, so the CLI contract is what is tested.
"""

import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest

HERE = pathlib.Path(__file__).resolve().parent
CHECK = HERE.parent / "state-check"

SPEC = textwrap.dedent(
    """\
    # Demo design

    ## Acceptance Examples
    ### R1 - Sync removes duplicate entries
    - consumer: end user
    - entry: `tool sync`
    - example: given [a, a, b] -> `tool sync` -> store [a, b]; stdout "2 items"
    - edge: empty inbox -> stdout "0 items"
    - non-goal: cross-device dedupe
    """
)

TEST_FILE = textwrap.dedent(
    """\
    import sys

    def test_a():
        return True

    def test_b():
        return True

    def main():
        test_a()
        test_b()

    if __name__ == "__main__":
        main()
    """
)

TOOL = "#!/bin/sh\necho 2 items\n"

STATE = textwrap.dedent(
    """\
    # STATE: demo
    schema: sdlc-state/v1
    spec: docs/superpowers/specs/demo-design.md
    plan: docs/superpowers/plans/demo.md
    ledger: docs/superpowers/progress.md
    test_cmd: python3 {path} -q {name}
    beads: off

    ## Resume
    - next: S01

    ## Requirements -> Evidence
    | REQ | Behavior (spec) | Consumer | Entry | Tests | W-check | Status |
    |---|---|---|---|---|---|---|
    | R1 | sync dedupes | end user | `tool sync` | tests/test_sync.py::test_a, tests/test_sync.py::test_b | S02 | covered |

    ## Touchpoints
    | T | File | Must contain | For REQ | Status |
    |---|---|---|---|---|
    | T1 | README.md | `tool sync` | R1 | done |

    ## Steps
    - [x] S01 Build sync | kind: build | req: R1 | est: 20m
      - check: `python3 tests/test_sync.py -q test_a`
      - expect: exit 0
      - evidence: 3f2a1c9 \u00b7 2026-11-15 02:10 \u00b7 exit 0 \u00b7 "ok"
    - [x] S02 Wire the CLI | kind: wire | req: R1 | est: 10m
      - check: `./tool sync`
      - expect: `2 items`
      - evidence: 3f2a1c9 \u00b7 2026-11-15 02:12 \u00b7 exit 0 \u00b7 "2 items"
    """
)

LEDGER = textwrap.dedent(
    """\
    # progress
    2026-11-15 02:00 | R1 | REQ-APPROVED | spec appendix approved | by: human
    2026-11-15 02:10 | S01 | DONE | 3f2a1c9 | exit 0
    2026-11-15 02:12 | S02 | DONE | 3f2a1c9 | exit 0
    """
)


class Project:
    def __init__(self):
        self.dir = pathlib.Path(tempfile.mkdtemp(prefix="state-check-"))
        self.write("docs/superpowers/specs/demo-design.md", SPEC)
        self.write("docs/superpowers/plans/demo.md", "# plan\n")
        self.write("docs/superpowers/progress.md", LEDGER)
        self.write("tests/test_sync.py", TEST_FILE)
        self.write("README.md", "run `tool sync`\n")
        self.write("tool", TOOL, mode=0o755)
        self.write("docs/superpowers/state/demo/STATE.md", STATE)

    def write(self, rel, text, mode=0o644):
        path = self.dir / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        path.chmod(mode)

    def patch(self, rel, old, new):
        path = self.dir / rel
        text = path.read_text()
        assert old in text, f"fixture is stale: {old!r} not in {rel}"
        path.write_text(text.replace(old, new, 1))

    def run(self, *args, stdin=None):
        env = dict(os.environ)
        env.pop("GIT_DIR", None)
        env.pop("GIT_WORK_TREE", None)
        return subprocess.run(
            [sys.executable, str(CHECK), *args],
            cwd=str(self.dir), capture_output=True, text=True, input=stdin, env=env,
        )

    def close(self):
        shutil.rmtree(self.dir, ignore_errors=True)


class Base(unittest.TestCase):
    def setUp(self):
        self.p = Project()

    def tearDown(self):
        self.p.close()

    def assertFinding(self, result, needle):
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(needle, result.stderr, result.stderr)


class TestStruct(Base):
    def test_no_state_files_is_ok(self):
        (self.p.dir / "docs/superpowers/state/demo/STATE.md").unlink()
        r = self.p.run("--struct")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("0 state files", r.stdout)

    def test_good_project_passes(self):
        r = self.p.run("--struct")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("ok (1 state file)", r.stdout)

    def test_good_project_final(self):
        r = self.p.run("--struct", "--final")
        self.assertEqual(r.returncode, 0, r.stderr)

    def test_req_drift_between_spec_and_state(self):
        self.p.patch(
            "docs/superpowers/specs/demo-design.md",
            "### R1 - Sync removes duplicate entries",
            "### R2 - Something else",
        )
        self.assertFinding(self.p.run("--struct"), "REQ drift")

    def test_examples_outnumber_tests(self):
        self.p.patch(
            "docs/superpowers/state/demo/STATE.md",
            "tests/test_sync.py::test_a, tests/test_sync.py::test_b",
            "tests/test_sync.py::test_a",
        )
        self.assertFinding(self.p.run("--struct"), "example/edge lines")

    def test_missing_test_file(self):
        self.p.patch(
            "docs/superpowers/state/demo/STATE.md",
            "tests/test_sync.py::test_b",
            "tests/test_absent.py::test_b",
        )
        self.assertFinding(self.p.run("--struct"), "test file not found: tests/test_absent.py")

    def test_planned_status_skips_test_existence(self):
        self.p.patch(
            "docs/superpowers/state/demo/STATE.md",
            "tests/test_sync.py::test_b",
            "tests/test_future.py::test_b",
        )
        self.p.patch(
            "docs/superpowers/state/demo/STATE.md",
            "| covered |",
            "| planned |",
        )
        self.assertEqual(self.p.run("--struct").returncode, 0)

    def test_consumer_facing_without_wire_step(self):
        self.p.patch(
            "docs/superpowers/state/demo/STATE.md",
            "| S02 | covered |",
            "| - | covered |",
        )
        self.assertFinding(self.p.run("--struct"), "W-check names no step")

    def test_wire_step_must_invoke_entry(self):
        self.p.patch(
            "docs/superpowers/state/demo/STATE.md",
            "- check: `./tool sync`",
            "- check: `./tool other`",
        )
        self.assertFinding(self.p.run("--struct"), "does not invoke entry")

    def test_wire_step_must_have_kind_wire(self):
        self.p.patch(
            "docs/superpowers/state/demo/STATE.md",
            "S02 Wire the CLI | kind: wire",
            "S02 Wire the CLI | kind: build",
        )
        self.assertFinding(self.p.run("--struct"), "must have kind: wire")

    def test_touchpoint_literal_missing(self):
        self.p.patch("README.md", "run `tool sync`\n", "run the tool\n")
        self.assertFinding(self.p.run("--struct"), "not in README.md")

    def test_touchpoint_none_needs_reason(self):
        self.p.patch(
            "docs/superpowers/state/demo/STATE.md",
            "| T1 | README.md | `tool sync` | R1 | done |",
            "| T1 | none |  | R1 | open |",
        )
        self.assertFinding(self.p.run("--struct"), "needs a reason")

    def test_missing_plan_file(self):
        (self.p.dir / "docs/superpowers/plans/demo.md").unlink()
        self.assertFinding(self.p.run("--struct"), "plan not found")

    def test_missing_ledger_at_final(self):
        (self.p.dir / "docs/superpowers/progress.md").unlink()
        self.assertFinding(self.p.run("--struct", "--final"), "ledger not found")

    def test_done_without_evidence(self):
        self.p.patch(
            "docs/superpowers/state/demo/STATE.md",
            '- evidence: 3f2a1c9 \u00b7 2026-11-15 02:10 \u00b7 exit 0 \u00b7 "ok"',
            "- evidence:",
        )
        self.assertFinding(self.p.run("--struct"), "without evidence")

    def test_step_without_check(self):
        self.p.patch(
            "docs/superpowers/state/demo/STATE.md",
            "  - check: `python3 tests/test_sync.py -q test_a`",
            "  - check:",
        )
        self.assertFinding(self.p.run("--struct"), "no check command")

    def test_estimate_over_ceiling(self):
        self.p.patch(
            "docs/superpowers/state/demo/STATE.md",
            "req: R1 | est: 20m",
            "req: R1 | est: 45m",
        )
        self.assertFinding(self.p.run("--struct"), "exceeds the 30 minute ceiling")

    def test_unknown_kind(self):
        self.p.patch(
            "docs/superpowers/state/demo/STATE.md",
            "kind: build | req: R1",
            "kind: nonsense | req: R1",
        )
        self.assertFinding(self.p.run("--struct"), "kind `nonsense`")

    def test_split_without_children(self):
        self.p.patch(
            "docs/superpowers/state/demo/STATE.md",
            "- [x] S01 Build sync",
            "- [~] S01 Build sync",
        )
        self.assertFinding(self.p.run("--struct"), "split but has no children")

    def test_last_child_must_carry_parent_check(self):
        self.p.patch(
            "docs/superpowers/state/demo/STATE.md",
            "  - evidence: 3f2a1c9 \u00b7 2026-11-15 02:10 \u00b7 exit 0 \u00b7 \"ok\"",
            "  - evidence: 3f2a1c9 \u00b7 2026-11-15 02:10 \u00b7 exit 0 \u00b7 \"ok\"\n"
            "  - [ ] S01a Do the first half | kind: build | req: R1 | est: 15m\n"
            "    - check: `python3 tests/test_sync.py -q test_a`\n"
            "    - expect: exit 0\n"
            "  - [ ] S01b Do the second half | kind: build | req: R1 | est: 15m\n"
            "    - check: `true`\n"
            "    - expect: exit 0",
        )
        self.p.patch(
            "docs/superpowers/state/demo/STATE.md",
            "- [x] S01 Build sync",
            "- [~] S01 Build sync",
        )
        self.assertFinding(self.p.run("--struct"), "must carry the parent's check")

    def test_dropped_without_ruling(self):
        self.p.patch(
            "docs/superpowers/state/demo/STATE.md",
            "- [x] S01 Build sync",
            "- [-] S01 Build sync",
        )
        self.assertFinding(self.p.run("--struct"), "dropped without a RULING")

    def test_dropped_with_ruling_passes(self):
        self.p.write(
            "docs/superpowers/progress.md",
            LEDGER + "2026-11-15 03:00 | S01 | RULING | dropped | by: human\n",
        )
        self.p.patch(
            "docs/superpowers/state/demo/STATE.md",
            "- [x] S01 Build sync",
            "- [-] S01 Build sync",
        )
        r = self.p.run("--struct")
        self.assertEqual(r.returncode, 0, r.stderr)

    def test_final_blocks_open_steps_and_uncovered_reqs(self):
        self.p.patch(
            "docs/superpowers/state/demo/STATE.md",
            "  - evidence: 3f2a1c9 \u00b7 2026-11-15 02:12 \u00b7 exit 0 \u00b7 \"2 items\"",
            "  - evidence:",
        )
        self.p.patch(
            "docs/superpowers/state/demo/STATE.md",
            "- [x] S02 Wire the CLI",
            "- [ ] S02 Wire the CLI",
        )
        self.p.patch(
            "docs/superpowers/state/demo/STATE.md",
            "| S02 | covered |",
            "| S02 | built |",
        )
        r = self.p.run("--struct", "--final")
        self.assertEqual(r.returncode, 1)
        self.assertIn("still [ ] at final", r.stderr)
        self.assertIn("at final (must be covered)", r.stderr)


class TestRun(Base):
    def test_run_passes_on_good_project(self):
        r = self.p.run("--run")
        self.assertEqual(r.returncode, 0, r.stderr)

    def test_run_catches_failing_mapped_test(self):
        self.p.write(
            "tests/test_sync.py",
            "import sys\n\n\ndef test_a():\n    sys.exit(3)\n\n\ndef test_b():\n    return True\n\n\n"
            "if __name__ == \"__main__\":\n    test_a()\n",
        )
        r = self.p.run("--run")
        self.assertEqual(r.returncode, 1, r.stdout)
        self.assertIn("exited 3", r.stderr)

    def test_run_catches_failing_wire_check(self):
        self.p.write("tool", "#!/bin/sh\necho broken\nexit 7\n", mode=0o755)
        r = self.p.run("--run")
        self.assertEqual(r.returncode, 1)
        self.assertIn("wire S02", r.stderr)

    def test_run_checks_wire_expect_literal(self):
        self.p.write("tool", "#!/bin/sh\necho 9 items\n", mode=0o755)
        r = self.p.run("--run")
        self.assertEqual(r.returncode, 1)
        self.assertIn("output lacks `2 items`", r.stderr)

    def test_run_catches_fabricated_s_step(self):
        # An [x] step's recorded evidence is self-attested until --run
        # re-executes its check. Break S01's check; the gate must fail.
        self.p.patch(
            "docs/superpowers/state/demo/STATE.md",
            "- check: `python3 tests/test_sync.py -q test_a`",
            "- check: `false`",
        )
        r = self.p.run("--run")
        self.assertEqual(r.returncode, 1, r.stdout)
        self.assertIn("S01", r.stderr)

    def test_struct_rejects_recursive_wire_check(self):
        self.p.patch(
            "docs/superpowers/state/demo/STATE.md",
            "- check: `./tool sync`",
            "- check: `python3 scripts/state-check --run`",
        )
        self.assertFinding(self.p.run("--struct"), "must not contain `--run`")

    def test_resume_last_green_must_match_final_green(self):
        self.p.write(
            "docs/superpowers/progress.md",
            LEDGER + "2026-11-15 03:00 | - | FINAL-GREEN | deadbee | ok |\n",
        )
        self.p.patch(
            "docs/superpowers/state/demo/STATE.md",
            "- next: S01",
            "- next: none\n- last-green: 3f2a1c9",
        )
        self.assertFinding(self.p.run("--struct"), "last-green")


class TestDoctor(Base):
    def test_doctor_reports_every_missing_gate(self):
        r = self.p.run("doctor")
        self.assertEqual(r.returncode, 1)
        self.assertIn("pre-push gate missing", r.stderr + r.stdout)
        self.assertIn("CI gate missing", r.stderr + r.stdout)

    def test_doctor_ok_when_gates_installed(self):
        self.p.write(".githooks/pre-push", "#!/bin/sh\npython3 scripts/state-check --run\n", mode=0o755)
        self.p.write(".github/workflows/state.yml", "jobs:\n  state:\n    steps:\n      - run: python3 scripts/state-check --run\n")
        r = self.p.run("doctor")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("doctor ok", r.stdout)

    def test_doctor_does_not_require_the_claude_stop_hook(self):
        # The Stop hook fires only under Claude Code. A project driven by another
        # harness must not be told its gate is missing when it never had one.
        self.p.write(".githooks/pre-push", "#!/bin/sh\npython3 scripts/state-check --run\n", mode=0o755)
        self.p.write(".github/workflows/state.yml", "jobs:\n  state:\n    steps:\n      - run: python3 scripts/state-check --run\n")
        r = self.p.run("doctor")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertNotIn("Stop hook", r.stdout + r.stderr)

    def test_doctor_still_fails_on_a_legacy_stop_hook_alone(self):
        # A Stop hook is not a substitute for the harness-neutral gates.
        self.p.write(".claude/settings.json", '{"hooks": {"Stop": [{"hooks": [{"command": "state-check --struct --hook"}]}]}}')
        r = self.p.run("doctor")
        self.assertEqual(r.returncode, 1)
        self.assertIn("pre-push gate missing", r.stderr + r.stdout)
        self.assertIn("CI gate missing", r.stderr + r.stdout)


class TestHook(Base):
    def test_hook_exits_two_on_findings(self):
        self.p.patch(
            "docs/superpowers/state/demo/STATE.md",
            '- evidence: 3f2a1c9 \u00b7 2026-11-15 02:10 \u00b7 exit 0 \u00b7 "ok"',
            "- evidence:",
        )
        r = self.p.run("--struct", "--hook", stdin="{}")
        self.assertEqual(r.returncode, 2)

    def test_hook_is_quiet_when_already_active(self):
        self.p.patch(
            "docs/superpowers/state/demo/STATE.md",
            '- evidence: 3f2a1c9 \u00b7 2026-11-15 02:10 \u00b7 exit 0 \u00b7 "ok"',
            "- evidence:",
        )
        r = self.p.run("--struct", "--hook", stdin='{"stop_hook_active": true}')
        self.assertEqual(r.returncode, 0)

    def test_hook_passes_clean_state(self):
        r = self.p.run("--struct", "--hook", stdin="{}")
        self.assertEqual(r.returncode, 0, r.stderr)


if __name__ == "__main__":
    unittest.main()
