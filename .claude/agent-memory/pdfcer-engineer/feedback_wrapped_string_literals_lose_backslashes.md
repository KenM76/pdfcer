---
name: wrapped-string-literals-lose-backslashes
description: Never patch a Rust line-continuation backslash via a heredoc/python -c — it becomes a literal \n. Use the Edit tool. Happened four times in one session.
metadata:
  type: feedback
---

# Never patch a Rust `\`-continuation through a heredoc. Use Edit.

Rust wraps a long string literal with a **backslash at end of line**, which
strips the newline and the next line's leading whitespace:

```rust
"pdfce-cli: printing is available on Windows only in this build \
 (docs/decisions/003-distribution-posture.md §4.1)"
```

Patching that construct with `python - <<'PYEOF'` or `sed` **reliably
corrupts it**, in one of two ways:

1. The backslash is **lost**, so the two fragments concatenate with the
   second line's indentation baked in — a shipped message with a
   ten-space gap mid-sentence.
2. The backslash survives but the newline does not get escaped through the
   layers, so the source ends up containing a **literal `\n` two-character
   sequence** in the middle of the format string — which Rust then renders
   as a REAL newline at runtime.

**Why:** the payload crosses shell → heredoc → Python string literal → file,
and each layer has its own opinion about `\`. Getting `\\\n` to arrive as
backslash-plus-newline requires counting escapes correctly through all four,
and I have now got it wrong four times in a single session.

**How to apply:** the moment a patch touches a `\` at end of line inside a
Rust string — or any Rust escape at all — **stop and use the Edit tool**, or
write a script *file* with the Write tool (no shell layer). Never `sed`, never
`python - <<EOF`, never `echo`.

## The failure is invisible to every gate except one

`cargo build`, `cargo clippy` and `cargo fmt --check` are all **completely
silent** on both corruptions: the literal is still a valid literal, it just
says the wrong thing. Nothing type-checks a sentence.

The only thing that has ever caught it is `pdfce-cli`'s
**stable-stdout-line test**, and only because that test asserts stdout is
*exactly one LF-terminated line*. Outside that one contract the corruption
ships.

**So: after any edit to an operator-facing string, grep for it.**

```bash
# literal \n embedded mid-string (the runtime-newline form)
grep -rn '\\\\n' crates/*/src/*.rs | grep -v '"\\\\n"'
# ragged multi-space gap (the lost-backslash form)
grep -rn '"[^"]*[a-z,.] \{4,\}[a-zA-Z(]' crates/*/src/*.rs
```

The second sweep's hits are mostly `assert!` messages in tests, which are
harmless; the ones that matter are anything reachable by an operator.

## The specific trap: a "fix" that re-introduces the bug

On 2026-08-18 I found the ten-space-gap form in `cmd_list_printers`' message,
fixed it **with a heredoc**, and thereby created the literal-`\n` form in the
same string. It shipped in that state and was caught hours later by the
stable-line test failing for an unrelated reason. **Repairing this bug with
the tool that causes it is the actual trap**, not the original mistake.

## ★★ THE TRIGGER, NAMED — it is not "editing a literal", it is WRITING RUST THROUGH A HEREDOC

Recorded 2026-08-26 after doing it **twice more in one day**, both times while
believing the rule did not apply.

**The Bash tool eats exactly one backslash level.** `python - <<'PY'` looks
quoted and safe, and it is — for the *shell*. What arrives at Python is
already one level down, so `"\\n\\"` in what I typed becomes `\n\` in the
file only if I counted a level I did not know was there. Both misses:

1. A `format!` string in `main.rs` — I appended a continuation line and the
   trailing `\` vanished, splitting one stdout key across two lines.
2. `settings/mod.rs` — the same thing inside the paragraph a *previous*
   commit had just repaired.

**The generative rule, which is stronger than "be careful with literals":**
*if a heredoc's payload is Rust source, do not use a heredoc.* Write the
script to a file with the Write tool and run it, or use Edit directly. The
payload does not have to contain a literal for this to bite — it bites on any
`\` anywhere, including in a doc comment, a path or a regex.

The Write-a-script-file route works and is now the default for anything that
generates code: the Write tool does no escaping pass at all.

## ★ 2026-08-26 — it happened AGAIN, in the repair commit again, and the gate that exists for it was GREEN

Exactly the trap above, third occurrence, same session shape: a heredoc
edit to `settings/mod.rs` lost the trailing backslash on two `\n\`
continuations. Two new facts, both worth more than the reminder:

**1. `check-string-gaps.sh` did not see it, and its header claims no false
negatives.** That gate matches a run of 3+ spaces *between word characters on
one source line*, which silently assumes `rustfmt` FOLDED the broken
continuation into its successor. **`rustfmt` cannot fold across a raw newline
inside a literal**, so the gap survives as *leading* indentation with no word
character in front of it. The gate is now widened to flag a displaced `\n`.

⇢ **Generalisation worth carrying past this file:** a gate that recognises a
defect by its POST-FORMATTING shape misses every instance the formatter was
unable to reshape.

**2. Round-trip tests cannot see it either.** Every settings test was
write → parse → compare, and a stray blank line round-trips *perfectly*
because `parse` trims before checking for `#`. The output was malformed in a
way no existing test could observe. The fix is an assertion on **what is
written**, not on what survives a round trip:
`every_line_of_the_written_file_is_a_comment_a_setting_or_a_blank`.

**And test the repair by REPRODUCING the defect.** My first widening of the
gate had a wrong discriminator (it keyed on "line does not end in a
backslash") that let a real variant through. Re-reading the rule would not
have found it; applying both variants to the real file and re-running the
gate did, in about a minute.

## ★★ 2026-08-27 — SEVENTH occurrence, and I called it the second

Two facts, and the second is the one that matters.

**1. I under-counted my own history by five.** The dispatch I wrote said
*"that is the SECOND time this exact mechanism has bitten"*. The
cross-project file `D:\dev\rag\rust\a_python_heredoc_eats_the_backslash_
continuation_in_a_rust_string_literal.md` keeps a running count and says
**n = 7**. The librarian corrected it.

⇢ **Do not state a recurrence count from memory. The RAG file that tracks it
is one grep away, and a wrong count argues for the wrong response** — "twice"
sounds like bad luck, "seven times" is a process defect.

**2. This one landed in a TEST'S ASSERTION MESSAGE, which the suite is
structurally blind to.** Every prior instance was in shipped code. An
`assert!` message is read *only when the test fails*, so it cannot affect
whether the test passes — no amount of green tells you anything about it.
`check-string-gaps.sh` caught it, and this is exactly why that gate must
**not** be scoped to `src/`, however tempting "it is only a test" sounds.

**Also this session: the same shell layer swallowed a whole heredoc twice**
when the payload contained a stray quote — `unexpected EOF while looking for
matching '`. The failure was loud rather than silent, which was luck. Both
times the fix was the same: write the payload to a file with the Write tool.

★ And a *different* failure mode of the same habit, worth its own line:
**a multi-edit `python - <<PY` script that asserts, then writes at the end,
loses every edit when a later assert fails.** Three substitutions succeeded,
the fourth `assert` blew up, nothing was written, and I did not notice until
a later `grep` showed two of the four missing. **Write after each
substitution, or use Edit.**

## ★ And a sibling mistake with the same shape: `git checkout --`

Not a heredoc, but the same *class* — a shell command whose blast radius is
wider than the intent. Mid-session I ran `git checkout -- page.rs` to undo a
deliberate one-line sabotage, and it reverted **the entire file to HEAD**,
discarding forty minutes of edits I then had to re-apply from context.

**Never use `git checkout --` to undo a sabotage.** Copy the file to a backup
first (`cp x /tmp/x.bak`) and restore from that. It is one extra command and
it is scoped to what you actually changed.

Related: [[windows-paths-need-literal-edits]] — same root cause (backslashes
crossing shell layers), same fix (Edit, or a written script file).
[[feedback_a_gate_that_underreports_looks_green]] — the same class one level
up: a gate whose output is wrong reads as a gate that passed.
[[feedback_run_the_projects_own_gates]] — `check-string-gaps.sh` is the only
thing that has ever caught this, and it is not one of the gates I reach for
by habit.

**★ 2026-08-30 — A SECOND MECHANISM, AND THE TRAVELLING ADVICE DOES NOT COVER
IT.** Everything above is about a continuation backslash being **lost in
transit** (a heredoc, `sed`, an inline Python string). This one **survived
transit intact** and was destroyed afterwards:

`cargo fmt` **rejoined** a correctly-continued Rust string literal onto a
single line and left the eaten indentation in place as a **ten-space run**
inside the message. The source I wrote was right; the formatter made it wrong.

⇒ Two causes, one signature, and *"never use a heredoc"* prevents only one of
them. **The only reliable detector is the gate** —
`bash tools/check-string-gaps.sh` — and it must be run **after** `cargo fmt`,
not before, because before the fmt the literal is clean.

**How to apply:** in the shutdown sweep, order matters: `cargo fmt` **then**
`tools/run-gates.sh`. Running the gate first certifies a tree the formatter is
about to change.

## ★ 2026-09-05 — EIGHTH, and this one went through the WRITE tool, so the standing fix did not cover it

The patch script was written with the Write tool (no shell layer at all) and
STILL shipped two gaps. Mechanism: **Python itself** treats a backslash at end
of line inside a non-raw `"""…"""` string as a line continuation and deletes
both the backslash and the newline. The Rust `\`-continuation I typed was
eaten by the Python parser, not by bash.

⇒ "Write a script file" is necessary, not sufficient. In a Python patch
script, Rust source that contains ANY backslash must sit in a **raw** string
(`r"""…"""`) or spell the backslash `\`. `check-string-gaps.sh` caught both
gaps before commit — run it after `cargo fmt`, every time.
