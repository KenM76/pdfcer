#!/usr/bin/env bash
#
# run-gates.sh — run every local check CI runs, in the order that makes the
# result mean something. **Derived from the workflow, never hand-typed.**
#
# WHY THIS EXISTS
# ===============
#
# On 2026-08-27 CI went red on a job called "check that every code commit is
# filed", after a session in which a THIRTEEN-GATE SWEEP had been run green
# before every push. The two gates whose entire job is the thing being violated
# — `check-commits-filed.py` and `check-passes-filed.py` — were the two the
# sweep did not include.
#
# The sweep was hand-typed from memory. `check-ci-parity.py --list` had printed
# the authoritative list all along, and it was not consulted.
#
# ⇢ **A sweep that omits a gate is byte-indistinguishable from a green one.**
# That is the same shape this repo has already recorded against individual
# gates that under-report, pointed one level up: at the *set*, not at the
# member. The fix is not another gate. It is to stop retyping the list.
#
# ★ AND `run-all.sh` WAS CITED AS THOUGH IT ALREADY EXISTED.
# `check-string-gaps.sh`'s header says a slow gate "is a gate that gets
# skipped, which is the failure mode `run-all.sh` exists to prevent". No such
# file was on disk. That same header already carries a struck note about having
# cited `check-strong-text.sh` — a gate that was planned and never written —
# **twice**, and about a later filing then repeating the name three times on
# the strength of reading it there.
#
# So this is the third dangling gate reference from one file, and the argument
# generalises: **a reference inside a trusted document is indistinguishable
# from a real one until somebody runs `ls`.** This script makes the cited thing
# exist rather than striking another sentence.
#
# WHAT IT RUNS
# ============
#
# Exactly what `python tools/check-ci-parity.py --list` prints, which that
# script derives by classifying every `run:` command in
# `.github/workflows/*.yml`. Add a gate to CI and it appears here with no edit
# to this file. That is the whole design: **the CI-derived list is not stored
# here.**
#
# ★ THAT SENTENCE READ "there is ONE list and it is not stored here" AND IS
# NOW FALSE — corrected 2026-08-28, the day it went stale, by the librarian's
# sweep rather than by anyone reading it.
#
# One command IS named in this file: `check-history-not-rewritten.py`, the
# pre-flight below. It is here *because* it cannot be derived — CI cannot run
# it at all (see its own comment at the invocation site).
#
# ⇢ The correction matters more than the wording. **The header stated an
# invariant a future refactor would enforce, and obeying it deletes the
# gate.** A right-sounding rule resting on a fact that has changed is worse
# than a wrong one: it survives review, because the sentence reads true.
# Same shape as the `ARCHITECTURE.md` §3 survivor found the same night.
#
# Two commands from that list are skipped by default and both are named when
# skipped, never silently dropped:
#
#   * `cargo about generate` — rewrites `THIRD_PARTY_LICENSES.md` in the work
#     tree, which a *checking* sweep must not do. Run it deliberately when the
#     dependency set changes (project rule 13).
#   * `cargo test --workspace --all-features` and its `--no-default-features`
#     sibling are run, but `--all-features` can pull in feature-gated OCR
#     model paths; `--full` runs them, the default runs plain
#     `cargo test --workspace`.
#
# ORDER IS PART OF THE CONTRACT
# =============================
#
# **The filing gates run LAST**, after everything else has passed, because
# their answer depends on commits that do not exist yet when the sweep starts.
# A filing gate run first reports the state of a tree you are about to change,
# which is the failure this script was written for.
#
# The corollary, which is a standing note in this project: **exactly ONE
# unfiled commit may sit at the tip.** `check-commits-filed.py` defers the tip
# on purpose — a commit cannot cite its own hash — and demands every commit
# below it. So: commit the code, dispatch the librarian, commit the filing,
# THEN push. Two unfiled code commits is already a red CI run.
#
# EXIT
# ====
#
# `0` when every command passed. `1` with a numbered summary of what failed,
# in run order. It does NOT stop at the first failure: a sweep that aborts
# early tells you about one problem when you wanted to know about all of them,
# and re-running a ten-minute sweep per defect is how a sweep becomes a thing
# people skip.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 2

FULL=0
LIST_ONLY=0
for arg in "$@"; do
    case "$arg" in
        --full) FULL=1 ;;
        --list) LIST_ONLY=1 ;;
        -h|--help)
            sed -n '2,80p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "run-gates: unknown option $arg (try --help)" >&2
            exit 2
            ;;
    esac
done

# ---------------------------------------------------------------------------
# The list, from the workflow. Not stored here — see the header.
#
# `check-ci-parity.py --list` prints a header line and then one indented
# command per line, so the filter is "indented, non-empty".
# ---------------------------------------------------------------------------
mapfile -t ALL < <(python tools/check-ci-parity.py --list 2>/dev/null |
    sed -n 's/^  \(..*\)$/\1/p')

if [ "${#ALL[@]}" -eq 0 ]; then
    echo "run-gates: could not derive the command list from" >&2
    echo "           check-ci-parity.py --list — is python on PATH?" >&2
    exit 2
fi

# ---------------------------------------------------------------------------
# Partition into "everything else" and "the filing gates", so the latter run
# LAST regardless of where the workflow happens to list them. Anything skipped
# is announced by name.
# ---------------------------------------------------------------------------
FILING_RE='check-(commits|passes)-filed'
SKIP_RE='cargo about generate'

main=()
filing=()
skipped=()
for cmd in "${ALL[@]}"; do
    if [[ "$cmd" =~ $SKIP_RE ]]; then
        skipped+=("$cmd")
    elif [[ "$cmd" =~ $FILING_RE ]]; then
        filing+=("$cmd")
    elif [[ "$cmd" == *"--all-features"* && "$FULL" != "1" ]]; then
        # `--all-features` builds the feature-gated OCR paths, which is a
        # several-minute compile. `--full` includes them; the default swaps in
        # the plain workspace test rather than dropping coverage silently.
        if [[ "$cmd" == cargo\ test* ]]; then
            main+=("cargo test --workspace")
            skipped+=("$cmd  -- use --full; plain 'cargo test --workspace' ran instead")
        else
            main+=("$cmd")
        fi
    else
        main+=("$cmd")
    fi
done

if [ "$LIST_ONLY" = "1" ]; then
    printf 'run-gates would run, in order:\n\n'
    printf '  --- pre-flight, first; CI cannot check this one ---\n'
    printf '  python tools/check-history-not-rewritten.py\n\n'
    for c in "${main[@]}"; do printf '  %s\n' "$c"; done
    printf '\n  --- filing gates, last ---\n'
    for c in "${filing[@]}"; do printf '  %s\n' "$c"; done
    if [ "${#skipped[@]}" -gt 0 ]; then
        printf '\nskipped (named, never silent):\n'
        for c in "${skipped[@]}"; do printf '  %s\n' "$c"; done
    fi
    exit 0
fi

# R241 (394th filing): the three public-facing gates are bound to `git push`
# by `tools/hooks/pre-push`, but only when this clone's `core.hooksPath`
# points there -- git config is not versioned, so a fresh clone has no hook.
# Say so on every sweep; a first bad push is the wrong place to find out.
failed=()
# Compared by resolved directory: an absolute spelling of the same folder
# (something on this machine rewrites it that way) gates pushes just as well.
hooks_path="$(git config --get core.hooksPath 2>/dev/null)"
if [ -n "$hooks_path" ] && [ -d "$hooks_path" ] && [ -d tools/hooks ] \
    && [ "$(cd "$hooks_path" && pwd -P)" = "$(cd tools/hooks && pwd -P)" ]; then
    printf 'pre-push hook: ACTIVE (core.hooksPath = %s)
' "$hooks_path"
else
    # R241 clause 2: a sweep on a clone whose pushes are not gated is RED,
    # not merely informed -- the 395th filing recorded the first draft's
    # "prints and exits 0" as a variance from the rule, and this closes it.
    printf 'pre-push hook: NOT ACTIVE -- run: git config core.hooksPath tools/hooks
'
    failed+=("pre-push hook not active (git config core.hooksPath tools/hooks)")
fi

run_one() {
    local cmd="$1"
    printf '\n=== %s\n' "$cmd"
    if ! bash -c "$cmd"; then
        failed+=("$cmd")
    fi
}

# ---------------------------------------------------------------------------
# THE ONE CHECK CI STRUCTURALLY CANNOT PROVIDE, so it is named here rather
# than derived from the workflow.
#
# Every other command in this sweep asks a question about the TREE, which the
# server re-checks. `check-history-not-rewritten.py` asks about the
# relationship between this branch and the remote -- and by the time CI runs,
# the push has already happened. A pre-push check is the only place it can
# live, which is also why `check-ci-parity.py` does not know about it.
#
# It runs FIRST because its failure invalidates everything after it: a sweep
# that certifies a tree whose history has been rewritten is certifying
# something nobody else can see.
#
# Added 2026-08-28, after a SUBAGENT amended an already-pushed commit without
# announcing it. That instance was harmless -- identical tree, metadata only --
# and was found by accident, which is the part that made it worth a gate.
run_one "python tools/check-history-not-rewritten.py"

for cmd in "${main[@]}"; do run_one "$cmd"; done

printf '\n=== filing gates (last, because their answer depends on commits that\n'
printf '    did not exist when this sweep started)\n'
for cmd in "${filing[@]}"; do run_one "$cmd"; done

printf '\n'
if [ "${#skipped[@]}" -gt 0 ]; then
    printf 'run-gates: SKIPPED, deliberately —\n'
    for c in "${skipped[@]}"; do printf '  %s\n' "$c"; done
    printf '\n'
fi

if [ "${#failed[@]}" -eq 0 ]; then
    printf 'run-gates: PASS — %d command(s), including %d filing gate(s).\n' \
        "$(( ${#main[@]} + ${#filing[@]} + 1 ))" "${#filing[@]}"
    exit 0
fi

printf 'run-gates: FAILED — %d of %d command(s):\n\n' \
    "${#failed[@]}" "$(( ${#main[@]} + ${#filing[@]} + 1 ))"
i=1
for c in "${failed[@]}"; do
    printf '  %d. %s\n' "$i" "$c"
    i=$(( i + 1 ))
done
printf '\nRe-run one of them on its own for its full output.\n'
exit 1
