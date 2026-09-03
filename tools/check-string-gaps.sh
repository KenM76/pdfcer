#!/usr/bin/env bash
#
# check-string-gaps.sh — a wrapped string literal must not bake a gap into
# the sentence the operator reads.
#
# WHAT THIS GATE IS FOR
# =====================
#
# Rust continues a string literal across source lines with a trailing
# backslash, which eats the newline AND the next line's leading whitespace:
#
#     "the scale is given as a ratio. To set it by \
#      pointing at a dimension"                        -> "…by pointing at…"
#
# Drop the backslash and the literal is still valid, still compiles, still
# passes every test that does not compare it to a hand-written expectation —
# and now contains a run of spaces where the indentation was:
#
#     "the scale is given as a ratio. To set it by
#      pointing at a dimension"                        -> "…by / six spaces / pointing…"
#
# `rustfmt` then joins the two source lines, so what is left in the file is one
# long line with the gap sitting in the middle of it, looking deliberate.
#
# WHY NOTHING ELSE CATCHES IT
# ===========================
#
# This gate exists because on 2026-08-18 `pdfcer-core` reported finding SIX of
# these in its own shipped error messages, two of them live since `95c3416`,
# and named the reason nothing had caught them:
#
#   - `cargo fmt` does not reflow the CONTENTS of a string literal, so the gap
#     is invisible to the formatter that reformatted the line around it;
#   - clippy has no lint for it;
#   - a mirror test that asserts two copies of the string agree compares one
#     broken copy against another and passes.
#
# The same grep over this workspace found **36 across 22 files**, eight of them
# in `crates/pdfce-gui/src/text/` — copy an operator reads on screen, including
# every sentence of the Set-scale dialog written the same afternoon. So this is
# not a defect one author makes once; it is what the language's line-
# continuation syntax does when a hand-edit loses one character, and the whole
# failure mode is that the result looks fine in the diff and wrong in the app.
#
# ★ AND IT IS INVISIBLE FROM INSIDE THE EDITOR.
#
# That is the property that makes it worth a gate rather than a note. Reviewing
# the source you see a wrapped sentence; the six spaces are indentation, which
# is exactly what your eye is trained to skip. It only becomes visible in the
# rendered window — which is R1's whole point, and R1 does not scale to every
# string in the tree. A grep does.
#
# WHAT COUNTS AS A VIOLATION
# ==========================
#
# Three or more consecutive spaces between two word-ish characters, on a line
# containing a double quote, outside a comment. Three rather than two because
# two spaces after a full stop is a typographic convention somebody may hold
# deliberately, and this gate should not adjudicate that.
#
# ★ INSIDE A `#[error(...)]` ATTRIBUTE, "WORD-ISH" ALSO INCLUDES `{` AND `}` —
# a format placeholder is a word THERE. Widened 2026-08-20 after this gate
# reported TWO of the THREE gaps a single `Pass` introduced. The one it could
# not see was
#
#     "...perimeter needs at least          {minimum}"
#
# because the character after the run was `{` and the class only admitted a
# letter. `thiserror`'s `#[error(...)]` messages are dense with placeholders, so
# a gap adjacent to one is not an exotic case — it is the common one, and the
# `}` end of the same class had the identical hole
# (`"...{remaining}          and..."`).
#
# ★ NOTE HOW IT WAS FOUND, because the shape recurs in this project: NOT by the
# gate reporting anything. By somebody knowing there were three defects and
# reading a report that listed two. A gate that under-reports is
# byte-indistinguishable from a green one, so the only detector is an
# independent forecast — which is the exact labour a gate exists to remove.
# `docs/NEXT_SESSION.md` §3 records the same shape against
# `check-ledger-numbers.py`'s star anchor, twice. The first fix there repaired
# the one spelling that had been seen rather than the class; this one widens
# both ends of the class rather than only the end that failed.
#
# ★★ AND WHY THE WIDENING IS SCOPED TO `#[error(...)]` RATHER THAN APPLIED
# EVERYWHERE. The first attempt widened the class globally and the scan went
# from 0 findings to about SIXTY, every one of them a deliberately aligned
# report column in a dev tool — `println!("files scanned:        {}", …)`. That
# is the shape the original narrow class was protecting, and nothing in the
# header said so, which is how it came to look like an oversight rather than a
# choice.
#
# The distinguishing property is not the characters. It is that a `thiserror`
# message is PROSE — a sentence an operator reads — while a `println!` in a
# sweep tool is a TABLE. Prose has no reason to contain a run of spaces; a
# table is made of them. So the gate widens exactly where prose is
# structurally guaranteed and stays narrow everywhere else, and the
# aligned-column case is now pinned in the clean self-test so a future
# widening cannot quietly re-break it.
#
# ★★★ A `pdfcer:` / `pdfce-gui:` DIAGNOSTIC IS PROSE TOO — widened
# 2026-08-27, and this is the THIRD time this one gate has missed by admitting
# only the spelling somebody had already seen. It reported PASS on a shipped
# refusal reading
#
#     "pdfcer: format-text needs --find TEXT, or --pin-span START:LEN with
#      an empty              --find to mean the whole pinned show operator"
#
# TWO holes at once, which is why it is worth its own note. The literal was not
# recognised as prose (it is an `eprintln!`, not a `#[error(...)]`), and even in
# prose mode the trailing class did not admit a HYPHEN — and the next word after
# a gap in a CLI diagnostic is, more often than anything else, a `--flag`.
#
# The structural argument is the same one that scoped the `{}` widening: a
# literal opening with the binary's own diagnostic prefix is a sentence an
# operator reads, never an aligned report column. `/` was added with `-`, for a
# path, before it cost a cycle to discover.
#
# ⇢ Measured before widening: a blanket trailing-class widening produced 24
# findings, ~18 of them legitimate (DXF byte streams, aligned probe columns).
# Scoped to prose it produced exactly 1 — the defect. The scoping is the whole
# design, not a compromise.
#
# ⇢ And the transferable part, now that it is three for three: in prose mode
# the trailing class should be read as "anything that starts a word", not as a
# list of the characters that have failed so far. Each of the three repairs
# enumerated from the instance in hand.
#
# WHAT THIS GATE CANNOT SEE, AND THE ONE ESCAPE HATCH
# ===================================================
#
# It reads text, not a syntax tree, so a line with a quote anywhere on it is
# treated as carrying a literal. Comments are stripped first, which removes the
# aligned tables in `egui-shell`'s manifest docs and every `#[expect(reason =
# …)]` justification — none of which an operator ever reads. The direction of
# the error is a deliberate trade: a false positive costs one reflow, and there
# is no false NEGATIVE that ships anything inert.
#
# ★ THAT SENTENCE USED TO CITE `check-strong-text.sh`, WHICH DOES NOT EXIST.
# So did the one further down about source lines versus code lines. No such
# gate is on disk and `git log --all -- 'tools/check-strong-text*'` returns
# nothing — it was planned and never written, and this header cited it twice
# as though it were a settled precedent. A later filing then repeated the name
# three times, on the strength of reading it here.
#
# ⇢ A dangling reference inside a trusted document is indistinguishable from a
# real one until somebody runs `ls`. The arguments were sound; only their
# attribution was invented, so they are now stated on their own account. If
# that gate is ever written, cite it then.
#
# A literal that genuinely needs the run of spaces — a test fixture holding
# escaped Rust source, an aligned report column — says so with a comment
# containing `string-gap-exempt:` and a reason. There is exactly one in the
# tree today, in `icons/glyphs.rs`, and it holds the input to the glyph
# scanner's own test.
#
# ★ THE MARKER MAY SIT IN THE COMMENT BLOCK ABOVE, NOT ONLY ON THE LINE.
#
# The obvious spelling is a trailing same-line comment, and that works. But a
# line long enough to trip this gate is already long, and R5 says the reason a
# rule is being set aside is exactly the kind of thing this project writes at
# length. A one-line-only marker would mean the better-documented exemption
# fails a gate the terse one passes — a backwards incentive that would push
# the next person to shorten the explanation to appease the tool. (This is the
# second of the two sentences that cited a gate which was never written; see
# the note above.)
#
# So the marker arms the NEXT code line, and blank and comment-only lines in
# between hold the arming. It covers one line, deliberately: an exemption that
# leaked down a file would silence violations nobody had looked at.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 2

SELF_TEST=0
[ "${1:-}" = "--self-test" ] && SELF_TEST=1

EXEMPT='string-gap-exempt'

# ---------------------------------------------------------------------------
# The scan — ONE `awk` PASS, NOT A SHELL LOOP.
#
# The first cut of this gate ran `sed` and `grep` as subprocesses per LINE. On
# Windows that is tens of thousands of process spawns and it did not finish in
# two minutes. **A gate slow enough to skip is a gate that gets skipped**,
# which is the failure mode `run-gates.sh` exists to prevent — so the shape of
# the implementation is part of the gate working, not an optimisation. `awk`
# reads every file in one process and the whole scan is well under a second.
#
# ★ THAT SENTENCE NAMED `run-all.sh`, WHICH DID NOT EXIST — the THIRD dangling
# gate reference from this one header (the `check-strong-text.sh` note below
# records the first two). It was MADE TRUE rather than struck:
# `tools/run-gates.sh` was written 2026-08-27, after a hand-typed
# thirteen-gate sweep omitted FIVE of CI's commands — including BOTH filing
# gates — and CI went red on one of them. It derives its list from
# `check-ci-parity.py --list`, so a sweep cannot be retyped short again.
#
# Comments are stripped BEFORE the match rather than the line being skipped
# when it has a comment on it, because a shipped literal routinely carries a
# trailing `// ui-text-exempt:` note. Stripping first means a gap in the code
# half is still found on a line whose comment half is clean.
# ---------------------------------------------------------------------------
scan() {
    local root="$1"
    local out
    out="$(find "$root" -name '*.rs' -not -path '*/target/*' -print0 2>/dev/null |
        xargs -0 -r awk -v exempt="$EXEMPT" '
            FNR == 1 { armed = 0; in_error = 0 }
            index($0, exempt) { armed = 1; next }
            {
                code = $0
                sub(/\/\/.*$/, "", code)
                if (code !~ /[^[:space:]]/) next    # blank or comment-only: hold the arming
                if (armed) { armed = 0; next }      # the marker above covers THIS line

                # PROSE MODE. A `#[error(...)]` message is a sentence an
                # operator reads, never an aligned report column, so inside one
                # a `{placeholder}` counts as a word and the class widens at
                # both ends. Stays armed across a multi-line attribute, which is
                # the form the three 2026-08-20 misses were written in.
                was_error = in_error
                if (index(code, "#[error(")) in_error = 1
                # ★ A CLI DIAGNOSTIC IS PROSE TOO — widened 2026-08-27 after
                # this gate reported PASS on a shipped `pdfcer` refusal
                # carrying fourteen baked spaces. Same structural argument as
                # `#[error(...)]`: a literal opening with the CLI
                # diagnostic prefix is a sentence an operator reads, never an
                # aligned report column. Measured tree-wide before widening:
                # 1 finding, the defect.
                cli = index(code, "\"pdfcer: ")
                prose = (in_error || was_error || cli)
                if (in_error && index(code, ")]")) in_error = 0

                # ★ THE DISPLACED ESCAPE — added 2026-08-26 after this gate
                # was GREEN on an instance of the very family it exists for.
                #
                # When a `\n\` continuation loses only its TRAILING backslash,
                # the `\n` is left stranded at the start of the NEXT source
                # line. That is the same defect and it ships the same garbage
                # — a raw newline plus the indentation of the source itself,
                # into a file the operator reads — but the spaces end up at
                # the START of the line, with no word character in front of
                # it, so the class below cannot match it.
                #
                # ⇢ The reason it was invisible is worth stating, because it
                # generalises: the class below assumes `rustfmt` FOLDED the
                # two lines together, and `rustfmt` cannot fold across a raw
                # newline inside a literal. A gate that recognises a defect by
                # its post-formatting shape misses every instance the
                # formatter was unable to reshape.
                #
                # ★ THE DISCRIMINATOR, and the first spelling of it was WRONG.
                #
                # The legitimate use is a line holding NOTHING BUT `\n\` — the
                # way a blank line is emitted inside a long literal. There are
                # four in the tree and every one is exactly that.
                #
                # The first attempt keyed on the line NOT ending in a
                # backslash, which sounded right and let a real variant
                # through: when the continuation is lost on one line but the
                # NEXT line keeps its own, the stranded `\n` sits in front of
                # text on a line that still ends in `\`. That variant ships
                # the same blank line and the same indentation. Caught only
                # because the repair was tested by REPRODUCING the defect
                # rather than by reading the rule.
                #
                # So: a displaced `\n` is any line whose first two characters
                # are the escape and which carries anything else besides the
                # continuation. Measured tree-wide: 4 legitimate, 0 flagged.
                body = code
                sub(/^[[:space:]]+/, "", body)
                if (substr(body, 1, 2) == "\\n" && body != "\\n\\" && body != "\\n") {
                    print "  " FILENAME ":" FNR ": a displaced `\\n` — the line ABOVE lost its trailing backslash"
                    print "      " substr(body, 1, 100)
                    next
                }

                if (code !~ /"/) next
                hit = 0
                if (code ~ /[A-Za-z,.:;)]   +[A-Za-z]/) hit = 1
                # ★ THE TRAILING CLASS ADMITS `-` AND `/` IN PROSE — widened
                # 2026-08-27, and the reason is the third instance of one
                # shape. The 2026-08-20 note above says the fix "widens BOTH
                # ENDS of the class rather than only the end that failed"; it
                # widened both ends to `{`/`}` and stopped there. The next
                # miss was
                #
                #     "…with an empty              --find to mean…"
                #
                # where the character after the run is a HYPHEN, because the
                # next word is a command-line flag — which is the single most
                # likely next word in a CLI diagnostic. `/` joins it for a
                # path, on the same reasoning and before it costs a cycle.
                #
                # ⇢ The transferable part is not the characters. It is that
                # "the class only admitted the spelling somebody had already
                # seen" has now happened three times to this one gate, and
                # each repair enumerated from the instance in hand. Prose has
                # no legitimate run of three spaces before ANY character, so
                # in prose mode the trailing class should be read as
                # "anything that starts a word", not as a list.
                if (prose && code ~ /[A-Za-z0-9,.:;)}]   +[-A-Za-z0-9{\/"]/) hit = 1
                if (hit) {
                    print "  " FILENAME ":" FNR ": a run of spaces baked into a string literal"
                    print "      " substr(body, 1, 100)
                }
            }
        ')"
    [ -z "$out" ] && return 0
    printf '%s\n' "$out"
    return 1
}

# ---------------------------------------------------------------------------
# Self-test — the discipline every gate here holds itself to.
#
# A gate that has only ever been seen to pass is indistinguishable from a gate
# that cannot fail, and this project has a recorded instance of exactly that:
# a deliberately planted violation `check-ui-strings.sh` did not detect, which
# briefly made a broken gate look like a working one.
#
# The clean fixture carries the four shapes most likely to produce a false
# positive: a doc comment aligning a table, a line comment mentioning the
# defect, an exempt literal, and a properly continued one — the correct
# spelling this gate exists to preserve.
# ---------------------------------------------------------------------------
if [ "$SELF_TEST" = "1" ]; then
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    mkdir -p "$tmp/dirty" "$tmp/dirty2" "$tmp/dirty3" "$tmp/clean" "$tmp/leak"

    cat > "$tmp/dirty/bad.rs" <<'EOF'
pub const fn note() -> &'static str {
    "With no line drawn yet, the scale is given as a ratio. To set it by      pointing at a stated length, measure it first."
}
EOF
    # The 2026-08-20 widening: a gap adjacent to a `{}` format placeholder, on
    # both sides. Each of these was invisible to the original character class,
    # and the second one shipped in a `thiserror` message.
    cat > "$tmp/dirty2/placeholder.rs" <<'EOF'
#[error("removing that vertex would leave {remaining}, and it needs at least          {minimum}")]
struct A;
EOF
    cat > "$tmp/dirty3/placeholder_lhs.rs" <<'EOF'
#[error("it has exactly {count}          picked points and cannot gain one")]
struct B;
EOF
    # The 2026-08-27 widening: a CLI diagnostic whose gap is followed by a
    # command-line FLAG. Both halves were holes — the literal was not
    # recognised as prose, and the trailing class did not admit a hyphen.
    mkdir -p "$tmp/dirty5"
    cat > "$tmp/dirty5/cli_flag.rs" <<'EOF'
fn refuse() {
    eprintln!("pdfcer: format-text needs --find TEXT, or --pin-span with an empty          --find");
}
EOF
    # The 2026-08-26 widening: a `\n\` continuation that kept its escape and
    # lost its trailing backslash, stranding the `\n` at the start of the next
    # line. This is the shape that shipped into a generated settings file
    # while this gate reported PASS.
    mkdir -p "$tmp/dirty4"
    cat > "$tmp/dirty4/displaced.rs" <<'EOF'
pub const fn note() -> &'static str {
    "# a comment line pdfcer writes to disk\n\
     # a second line, correctly continued.
\n             # a third, whose predecessor lost its backslash\n"
}
EOF
    cat > "$tmp/clean/good.rs" <<'EOF'
//! A doc comment may align a table:
//!     Mode(id: "read",   label: "Read")
pub const fn note() -> &'static str {
    // A comment describing the       baked-gap defect must not trip the gate.
    "With no line drawn yet, the scale is given as a ratio. To set it by \
     pointing at a stated length, measure it first."
}
pub const fn fixture() -> &'static str {
    "one two     three"  // string-gap-exempt: holds escaped Rust source
}
pub const fn block_marked() -> &'static str {
    // string-gap-exempt: the marker may sit in the comment block above, so
    // that the reason can be written at length.
    "one two     three"
}
pub const fn short() -> &'static str {
    "Two spaces after a stop.  That is a convention, not a defect."
}
/// The LEGITIMATE displaced escape: a line that is nothing but `\n\`, used to
/// emit a blank line inside a long literal. It ends in a backslash, which is
/// exactly what distinguishes it from the defect, and there are four of these
/// in the tree.
pub const fn blank_line_inside_a_literal() -> &'static str {
    "pdfce-gui — the desktop application.\n\
     \n\
     Usage: pdfce-gui [FILE]\n"
}
/// An ALIGNED REPORT COLUMN. Deliberate, ubiquitous in this repo's sweep
/// tools, and the reason the `{}`-aware class is scoped to `#[error(...)]`
/// instead of applied everywhere. A global widening turned this shape into
/// ~60 findings in one scan.
pub fn report(files_scanned: usize, clean: usize) {
    println!("files scanned:        {}", files_scanned);
    println!("  clean (strict load)   {}", clean);
}
EOF
    # An exemption must cover ONE line, not leak down the file.
    cat > "$tmp/leak/leak.rs" <<'EOF'
pub const fn marked() -> &'static str {
    // string-gap-exempt: this one is deliberate.
    "one two     three"
}
pub const fn unmarked() -> &'static str {
    "four five     six"
}
EOF

    fail=0
    if scan "$tmp/dirty" > /dev/null; then
        echo "SELF-TEST FAILED: a baked gap was not detected"
        fail=1
    fi
    if scan "$tmp/dirty2" > /dev/null; then
        echo "SELF-TEST FAILED: a gap before a {placeholder} was not detected"
        fail=1
    fi
    if scan "$tmp/dirty3" > /dev/null; then
        echo "SELF-TEST FAILED: a gap after a {placeholder} was not detected"
        fail=1
    fi
    if scan "$tmp/dirty4" > /dev/null; then
        echo "SELF-TEST FAILED: a displaced \\n (lost trailing backslash) was not detected"
        fail=1
    fi
    if scan "$tmp/dirty5" > /dev/null; then
        echo "SELF-TEST FAILED: a gap before a --flag in a CLI diagnostic was not detected"
        fail=1
    fi
    if ! scan "$tmp/clean"; then
        echo "SELF-TEST FAILED: a clean file was reported as a violation"
        fail=1
    fi
    # The arming must expire after one code line. If it leaked, the second
    # literal here would be silently exempt and the gate would go quiet
    # exactly where somebody had already used the escape hatch once.
    if scan "$tmp/leak" > /dev/null; then
        echo "SELF-TEST FAILED: an exemption leaked past the line it marks"
        fail=1
    fi
    [ "$fail" = "1" ] && exit 1
    echo "check-string-gaps self-test: PASS"
    exit 0
fi

echo "check-string-gaps: scanning crates/ and tools/ for baked-in gaps…"
found=0
for root in crates tools; do
    [ -d "$root" ] || continue
    scan "$root" || found=1
done

if [ "$found" = "1" ]; then
    cat <<'MSG'

A string literal contains a run of three or more spaces mid-sentence.

Almost always this is a line continuation that lost its trailing backslash:
the literal still compiles and the gap ships into whatever the operator reads.
Rejoin the sentence, or continue it properly with a trailing backslash — Rust
eats the newline and the next line's indentation.

If the spaces are wanted, say so on the same line with a trailing comment
containing `string-gap-exempt:` and the reason.
MSG
    exit 1
fi

echo "check-string-gaps: PASS — no baked-in gaps."
exit 0
