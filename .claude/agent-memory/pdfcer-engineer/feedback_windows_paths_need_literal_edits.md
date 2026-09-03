---
name: windows-paths-need-literal-edits
description: Never patch text containing ANY backslash (Windows paths, Rust/C escapes, line continuations) through a heredoc or sed — write a script file or use Edit; also, never `git checkout` to undo a sabotage
metadata:
  type: feedback
---

Edit any text containing a **Windows path** with a literal string tool
(Edit/Write), never through a `sed` expression or a Python/shell heredoc.

**Why:** on 2026-08-11 I rewrote `docs/NEXT_SESSION.md` through a Python
heredoc containing `D:\builds\pdfce-...`. Python read the `\b` as an escape,
ate the `b`, and wrote a literal **0x08 BACKSPACE byte** into the file. It
rendered as `D:uilds\...` — a path that does not exist, in the one document
whose job is to tell the next session where the build is.

It survived two readings because **a control character is invisible in normal
output.** It was found only by grepping for the *old* build hash expecting
zero matches and getting one — a search looking for staleness that turned up
corruption instead. Confirmed with `cat -v`, which is the tool that makes it
visible.

The exposure is wide, not a one-off: `\b`, `\t`, `\n`, `\f`, `\v`, `\0`, `\r`
and `\a` are all real escapes, and this project's own documents name
`D:\builds`, `D:\temp`, `D:\Dev\...`, `\fixtures`, `\target`. Any of those
after a backslash is a live grenade in a non-raw string.

**RECURRED 2026-08-28, in the SAME FILE, with `\t`.** Writing a corpus path
into `docs/NEXT_SESSION.md` through a Python heredoc turned `Dev` + backslash +
`temp` into `Dev` + a literal TAB + `emp`. Identical mechanism, different
escape, seventeen days later — and again in the one document whose job is to
orient the next session.

Two things this instance adds:

- **The memory did not prevent it.** I had read this file, and the failure
  still happened, because at the moment of writing I was thinking about the
  *content* (a scope decision) and not about the *transport*. The rule is
  known and is not self-applying.
- **★ WHAT ACTUALLY CAUGHT IT WAS A DIFFERENT GATE ENTIRELY.**
  `check-suite-name-absent.py` went red on the same edit — because the same
  paragraph had named private corpus FILES — and re-reading the block to fix
  *that* is what surfaced the tab. **A near-miss found as a side effect of an
  unrelated gate is a near-miss, not a save.** Without the filename mistake,
  the tab would have shipped exactly as the 0x08 did.

⇒ The durable form of this rule is not "remember"; it is **do not use a
heredoc for a path at all.** There is no case where `Edit` is worse.

**★★★ FOUR TIMES IN ONE SESSION (2026-08-27/28), and the fourth cost a
SABOTAGE CHECK.** Instances: a `\t` in a Windows path (a literal TAB into a
handoff); a `\n` in `.join()` (a raw newline inside a Rust string literal);
a `\u{FFFD}` in a test rewrite (Python rejected the file outright — the
loud, harmless case); and a `\u{FFFD}` in a sabotage patch, where the
heredoc died, **the sabotage was never applied, and the tests printed `ok`.**

★ That fourth one is the dangerous shape and it is why this note is being
rewritten rather than merely incremented. A sabotage that silently does not
happen looks EXACTLY like a sabotage the tests survived — a green run that
reads as "the suite is robust" when it means "nothing was tested". I only
caught it because the Python traceback was in the same output block as the
`ok` lines.

⇒ **If a sabotage reports the suite still green, check that the sabotage
actually landed before believing anything.** `grep` for the mutated token.

**THE RULE IS NOT WORKING AS A RULE.** I have read this file, I know its
content, and I violated it four times in eight hours — every time while
thinking about *what* to write rather than *how it travels*. So state it as a
mechanical default with no judgement in it:

> **Never pass a `\` through the Bash tool. Not in a heredoc, not in `sed`,
> not in an inline Python string. Use `Edit`/`Write`, which take literals.**

There is no case where `Edit` is worse. When a patch genuinely needs logic,
`Write` the script to a file first and run it — the file is not re-interpreted
by a shell.

**How to apply:**
- Paths → `Edit`/`Write`, always. They take literals; nothing is interpreted.
- If a heredoc is genuinely necessary, use a **raw** string (`r'''...'''`) or
  double every backslash — and then *verify with `cat -v`*, not by eye.
- After any bulk rewrite of a document, sweep for control characters:
  `python -c "d=open(F,encoding='utf-8').read(); print([hex(ord(c)) for c in set(d) if ord(c)<32 and c not in '\n\r\t'])"`

**★ WIDENED 2026-08-17 — it is not only PATHS, it is any backslash, and it
bit me three times in one session on SOURCE CODE.** The rule above scoped
the hazard to Windows paths; that scoping is what let me walk into it again.

- A Rust string **line-continuation** `\` at the end of a line inside a
  format string was eaten by a quoted heredoc (`<<'PYEOF'`), so the patch
  script's anchor never matched and the `assert` fired. Twice.
- Worse, once it *did* apply: I wrote `\\\n` intending a Rust continuation
  and produced a **literal `\n` escape** in the source, which Rust then
  compiled into a real newline. That broke `pdfce-cli render-page`'s
  one-line stdout contract — caught only because a contract test asserts
  `line.matches('\n').count() == 1`.

So the trigger is **a backslash in the payload**, whatever it means:
Windows paths, Rust/C string escapes, regex, LaTeX, `\|` in Markdown tables.

**How to apply, updated:** for any multi-line patch to source, **Write a
script file and run it**, or use `Edit` directly. Do not fight the heredoc —
the failure is silent when it is not loud, and the loud version costs a
build cycle.

**★ A SECOND, UNRELATED TRAP FROM THE SAME SESSION, filed here because it
also destroys work silently:** `git checkout <file>` to undo a **sabotage
check** reverts the file to `HEAD` — including the *feature work* you were
sabotaging, if it is not yet committed. I lost every change to
`crates/pdfce-render/src/color.rs` that way and had to re-apply them from
the patch scripts. **Copy the file aside before sabotaging**
(`cp x D:/Dev/temp/x_backup`) and restore from that copy, never from git.

**★★ AND I DID IT AGAIN ON 2026-08-18 — the scoping is what let me.** The
paragraph above says *"to undo a **sabotage** check"*, so when I wanted to
undo a **half-applied patch script**, the rule did not feel like it applied.
It is the same command with the same effect. I ran `git checkout -- crates/`
and lost three uncommitted edits to tracked files.

**The detail that makes this dangerous rather than merely annoying: it
spared every UNTRACKED file.** Three brand-new modules survived untouched
while three small edits to existing files vanished, so the tree looked ~90 %
intact and `cargo build` still nearly worked. Nothing announced the loss —
it surfaced as a compile error about a missing module, which reads like a
forgotten `mod` line, not like data loss.

**How to apply — the rule with no scope on it: NEVER use `git checkout`,
`git restore` or `git stash` to undo anything while uncommitted work is in
the tree.** Not for sabotage, not for a bad patch, not for "just this one
file". Undo by *editing the change back*, or by restoring from a copy you
made first. If a bulk revert genuinely seems necessary, commit the good work
first — a throwaway commit costs nothing and is reversible; a checkout is
not.

**★★★ 2026-08-20 — THREE MORE, IN ONE SESSION, ALL THE SAME PATH.** Even
with the rule already widened to "any backslash", I put
`D:\Dev\temp\pdfce\ncored-benchmark-cad-drawing.pdf` through a **quoted**
heredoc (`<<'PY'`) with **doubled** backslashes — the form this file says is
acceptable — and it still arrived as `D:\Dev<TAB>emp\pdfce<NEWLINE>cored-…`.
Twice into `docs/NEXT_SESSION.md` and once into an agent-memory file.

**So the escape processing is happening BEFORE bash sees the heredoc, and
quoting does not stop it.** `<<'PY'` protects against *bash* expansion; it
does not protect against the tool layer. The mitigation this file offered —
"double every backslash" — is therefore **not sufficient**, and the sentence
above about raw strings only helps if the payload survives to Python intact,
which it does not.

**The rule, with the escape hatch removed: a backslash never goes through the
Bash tool. Not doubled, not quoted, not raw.** Use `Write` for a new file or
`Edit` for a change; both take literals end to end.

**And the structural mitigation that actually worked:** the benchmark path now
appears **once**, on its own line, in `docs/NEXT_SESSION.md` §7, and every
other mention references that section instead of repeating it. One place to
get right beats seven places to check.

**★★ 2026-08-21 — WIDENED AGAIN, TO BACKTICKS, and the trigger is not a
backslash at all.** I wrote a commit message with `git commit -m "…"` that
contained a backticked phrase. **Bash read the backticks as COMMAND
SUBSTITUTION** and spliced a fifteen-line file listing into the middle of a
sentence. It shipped; I caught it only because I printed the message back.

Note the shape, because it is why the previous wording did not protect me:
this file had grown into *the backslash rule*, and a backtick is not a
backslash — so the rule did not feel like it applied. **The real class is
"content that goes through a shell gets interpreted by the shell,"** and the
dangerous characters are all of `` ` ``, `$`, `\`, `!`, and unbalanced quotes.

**Operationally, one line covers every instance so far: NEVER put prose
through the Bash tool.** Not a commit message, not a heredoc, not a `-m`.
Write the text with `Write` and pass the file (`git commit -F file`), or use
`Edit`. Every long commit message this session went through `-F` and was
fine; the ONE that used `-m` because it was "short" is the one that broke.

**★★ AND IT HAD HAPPENED HERE BEFORE — `5047cb9`, 2026-08-11, in CI.** A
double-quoted shell string in `ci.yml` contained `` `aes` ``, meant as a
code-span; bash ran it as a command, found nothing, and spliced the empty
output, so the error message printed *"pdfce-core  enables an extra feature
on the  crate"* — **losing the single word it existed to say.** `set -e` did
not catch it, because `echo`'s exit status is 0.

Same root cause, **different medium**: a CI error string then, a commit
message now. Two occurrences across two media is this project's promotion
bar, and the pair is more instructive than either alone — *the danger is not
a file format or a tool, it is that a shell reads its input.*

Note also how each was found: the 2026-08-11 one only because somebody
deliberately made the gate FAIL to see what it printed; today's only because
I printed the message back. **Neither would have surfaced from a green
run**, which is the property they share with the vacuous tests in
[[splice-end-marker-must-be-searched-from-start]]'s neighbourhood.

**★ 2026-08-22 — THREE MORE, and the new information is about the RETRY,
not the failure.** Writing the mitochondrion module and its README, three
`python - <<'EOF'` heredocs went wrong in two distinct modes:

- **Loud:** two heredocs whose payload was ordinary English prose died with
  `unexpected EOF while looking for matching "'"`. The payload contained
  nothing but an apostrophe in a word like *"cell's"*. Quoting the delimiter
  did not help, consistent with the 2026-08-20 finding that the escape
  processing happens above bash.
- **Silent:** a `\\n` written inside a Python string arrived as a **real
  newline**, so an anchor built from a Rust source line never matched and
  the `assert` fired. That is the 2026-08-17 mode again, unchanged.

**What is actually new: I retried instead of switching.** After the first
heredoc failed I re-ran a variant of the same approach, then debugged *why*
the anchor did not match, then finally used `Edit`. That cost four tool
calls to reach a conclusion this file already states in bold.

**How to apply — a reflex, not a judgement: the FIRST time a heredoc
misbehaves, stop and use `Write`/`Edit`.** Do not diagnose it, do not try a
different quoting form, do not double anything. The diagnosis is already
written above and it never changes. The loud mode is the lucky one; the same
call could have half-applied.

**★ 2026-08-25 — TWICE MORE, and what is new is WHO CAUGHT IT.** Two Rust
line continuations lost their trailing backslash to a heredoc, so two string
literals shipped with a ten-space hole mid-sentence. Same mode as 2026-08-17,
nothing new about the cause.

What is new: **`tools/check-string-gaps.sh` caught both.** This project now
owns a gate for exactly this defect, so the failure mode has moved from
"ships silently" to "fails a gate" — which is a real improvement and also a
trap, because it makes the heredoc feel survivable. It is not: the gate only
sees a *run of three or more spaces*, so a continuation lost at a word
boundary, or one that produces a doubled word rather than a gap, still ships.

★★ And the second one survived a full gate sweep. The sweep ran, went green,
and then the file was edited once more. **A gate answers a question about the
tree it was run on, not the tree that follows** — the same shape as the
`git ls-files` finding the same day, one scale down. Run the string gate
again after the LAST edit, not after the last batch of edits.

Operationally unchanged and now three-times reinforced: **every edit that went
through a script Written to a file was fine; every one that went through a
heredoc was not.**

Related: [[absence-needs-an-unscoped-query]] — same family. Both are cases
where a tool returned something that *looked* like a normal result, and the
only defence was checking with an instrument rather than with a glance. Also
[[splice-end-marker-must-be-searched-from-start]], from the same session:
another scripted patch, another silent corruption, found by a reader rather
than a check.

---

## ★★ 2026-08-27 — A FOURTH RECURRENCE, AND IT WAS A COMMIT MESSAGE

The same failure, in a place this note did not name: **`git commit -m` from the
shell**, with a prose message containing backticked identifiers.

Every backticked term was **command-substituted away**. `` `object` ``,
`` `editable=false` ``, `` `form_cycles` ``, `` `^object ` `` all became empty
strings. The commit succeeded. The message shipped with holes in the sentences:

> *"A SEPARATE LINE TYPE, NOT MORE  ROWS"* … *"An  index is what the editing
> subcommands take"* … *" is on every leaf row for the same reason."*

`bash` also printed `object: command not found` six times, which I read as noise
from an unrelated step rather than as the message being eaten.

**Why this one is worth appending rather than filing separately:** the existing
note is about **backslashes in file content**, and I had internalised it as a
rule about *editing files*. A commit message is not a file edit, so the rule did
not feel like it applied — the same shape as the harness note that read as a
"code-writing pre-flight" and was skipped twice for not being code.

**The generalisation, stated so it covers the next unnamed place:**

> **Any prose that reaches a shell as an argument is a hazard, whatever it is
> for.** Not just file content. Commit messages, `--message` flags, `echo`,
> heredocs, `printf`. Backticks and `$(...)` are substituted; backslashes are
> escapes.

**How to apply:** write the message to a file with `Write`, then
`git commit -F <file>`. This is already the habit for long messages and was
skipped here because the message felt short enough. It was not.

**And there is no gate for it** — a mangled commit message is a perfectly valid
commit message. The only detection is re-reading `git log -1 --format=%B` after
committing, which is cheap and is now the habit.

---

## ★★★ 2026-08-28 — THE INVERSE, AND THIS FILE'S OWN MITIGATION CAUSED IT

Every instance above is *a backslash being eaten*. This one is **a backslash
being preserved when it needed to be interpreted**, and it arrived through the
exact fix this file recommends.

I `Write`-ed a patch script to a file (correct — that is the rule) and used a
Python **raw** triple-quoted string `r'''...'''` for the Rust payload (also
recommended above, verbatim: *"use a raw string (`r'''...'''`)"*). The payload
contained `\u2014` for an em dash. Raw means **no interpretation**, so all 17
of them reached the Rust source as the literal six characters `\u2014`.

Two outcomes, and the second is the dangerous one:

- Inside a **Rust string literal** → hard compile error. Rust wants
  `\u{2014}` with braces. Loud, cheap, fixed in one pass.
- Inside a **`///` doc comment** → *compiles perfectly*. It would have shipped
  as operator-facing `--help` text reading `resize this Square \u2014 the
  placement matrix…`. No gate sees it: not clippy, not `check-string-gaps.sh`
  (which looks for runs of spaces), not the UI-strings gate (clap help is not
  a `ui_text.rs` literal).

⇒ **The two failure modes are opposite and the mitigations are opposite.** A
NON-raw string eats backslashes you meant to keep; a RAW string keeps
backslashes you meant to interpret. "Use a raw string" is not a fix, it is a
*different* bug.

**The rule that covers both without a judgement call: put the actual character
in the payload, never an escape for it.** Type `—`, `§`, `★`, `×` literally.
`Write` and `Edit` are UTF-8 end to end and never needed the escape; the escape
was a habit carried over from environments that did.

**Detection, since no gate exists:** a bare `\uXXXX` **not followed by `{`** is
never valid Rust. `grep -nP '\\u[0-9a-fA-F]{4}(?!\{)' <file>` — the negative
lookahead is what keeps genuine `\u{FFFD}` escapes out of the results.

★ Note the meta-shape, which is the reason this is appended rather than filed
apart: **this file has now recommended a mitigation that became the next
defect.** A remedy written against one direction of a hazard is not neutral
about the other direction, and a long rules document accumulates exactly that
kind of stale advice. The two paragraphs above recommending raw strings and
doubled backslashes were both later shown insufficient; they are kept visible
rather than deleted, but they are **not** current guidance.

---

## 2026-08-30 — it is not only PROSE. It ate a SABOTAGE SCRIPT.

The rule above is written about commit messages and error strings. The same
collapse hit a **verification script**, where the consequence is worse: a
sabotage that does not apply is **indistinguishable from a test that catches
nothing**.

Sabotaging a settings writer through a quoted bash heredoc, the search string
`\n` reached Python as a **real newline** rather than backslash-n, so it
matched nothing, the `.replace()` was a silent no-op, and the test suite
passed. I read that pass as *"the test is robust"*. It proved nothing at all.

**How to apply.** Two habits, and the second is the one that generalises:

1. Write any script containing a backslash with the **Write tool**, never a
   bash heredoc. Or build the escape at runtime — `chr(92) + "n"` — which no
   shell can touch.
2. ★ **Every sabotage asserts that it applied.** `assert s.count(old) == 1`
   before the replace. A sabotage is a measurement, and an instrument that
   silently failed to fire reads exactly like a negative result.

**The same run then produced a second lesson.** With the sabotage finally
applied, three tests failed — but by the *unknown-key* assertion, not the
value comparison, because renaming a key is not the failure mode being
claimed. Only OMITTING it is. **Sabotage the thing the comment actually
claims**, then run the **counterfactual** (old fixture + omitted key) to prove
the claim was not already covered by a sibling test. Here it was not: the old
fixture passed all 35, the corrected one fails.

Related: [[a-default-valued-fixture-cannot-falsify-a-carry]] is what the
corrected fixture is an instance of; [[sabotage-catches-false-comments]] is
why the counterfactual was worth running.


**★★ TWO MORE WAYS A PATCH SCRIPT DESTROYS WORK SILENTLY — both hit on
2026-08-30, both in the same afternoon, neither covered above.**

**(a) `pathlib.Path.write_text()` rewrites the WHOLE FILE to CRLF on Windows.**
Default `newline=None` translates every `\n` to `os.linesep`. A one-line
sabotage-and-restore script therefore converted all 40,699 lines of
`crates/pdfce-core/src/edit.rs` from LF to CRLF while restoring it
"identical" — `read_text() == original` compares *after* translation, so the
script's own assertion passed. `git diff --stat` looked normal (the line
counts were right); the only tell was git's `"CRLF will be replaced by LF"`
warning, which is easy to read as noise.

⇒ **Always pass `newline='\n'` to `write_text`**, and after any script that
rewrites a source file, check `python -c "print(open(F,'rb').read().count(b'\r\n'))"`
before committing. A whitespace-only 40k-line diff in a public repo is not
recoverable by apology.

**(b) A sabotage LOOP leaves the previous case applied when a later anchor
fails.** The loop wrote sabotage 2, ran it, then hit `assert n == 1` on case
3's anchor and raised — and the restore was *after* the loop, so it never
ran. `crates/pdfce-core/src/edit.rs` sat on disk with the grouping-node
refusal deleted, and the only reason it was noticed was habitually running
`git diff --stat` afterwards.

This is the same shape as the fourth instance above (a sabotage that never
applied looks like a suite that survived it) pointed the other way: **a
sabotage that never *un*-applied looks like working code.** And it is worse,
because the tests then pass — the sabotaged case is the one already proven to
fail, so re-running the whole file would have gone red, but re-running
anything else goes green.

⇒ **Restore in a `try`/`finally`, never after the loop**, and **validate every
anchor up front** before touching the file at all. Both are one line each.

**★ 2026-08-30, live instance while WRITING THIS FILE'S SIBLING.** A
`python -c "..."` one-liner containing backticks — ordinary Markdown code spans
in a memory-index line — was **command-substituted by bash**: `` `pub struct` ``
became an attempt to run `pub`, and the resulting string never matched, so the
edit silently did nothing while the heredoc in the *same command* succeeded.

Two things that makes concrete:

- **The hazard is not only the backslash.** A **backtick** inside a
  DOUBLE-quoted shell string is substitution, and Markdown prose is full of
  them. `python -c "..."` is therefore as unsafe as a heredoc, and it *looks*
  safer because it is one line.
- **A mixed command can half-succeed.** The `<<'MD'` heredoc appended
  correctly; the `python -c` beside it did not. **The visible output was a
  Python traceback for the second half only**, which reads as "that one edit
  failed" rather than "check what did land".

⇒ **`python -c` with any prose payload is the same mistake as a heredoc.**
Write the script to a file (`Write`), run it, delete it. That path has no shell
interpretation at all.

---

## 2026-08-31 — four more, one reached a commit, and a new medium

Nothing new about the cause; three things new about the surroundings.

**A committed GENERATOR SCRIPT.** `tools/gen-form-recursion-fixtures.py` builds
PDF fixtures from byte-string payloads full of escapes. A heredoc turned each
one into a real newline, and Python rejected the file outright. Loud, cheap —
but it is a medium this note had not named: a *tool* that writes *fixtures*,
two removes from the code being edited.

**One reached a commit and the gate caught it.** A Rust line continuation lost
its backslash inside a new `EditError` message; `check-string-gaps.sh` found it
during the sweep. **That is the gate earning its keep and it is also the trap
the 2026-08-25 entry warns about** — the gate only sees a run of three or more
spaces, and it fired here only because the continuation happened to be indented.

**★ THE VERIFICATION ITSELF WENT THROUGH THE SAME MANGLING.** After `Write`-ing
a payload to a temp file, I tried to confirm the continuations survived with
`python -c "... h.count('\\n') ..."` — a check whose own escape sequence is
subject to the identical hazard. It returned 0 on a file that was **fine**, I
believed it, and the `rm` in the same compound command then deleted the good
file before I could look.

⇒ **Check with `cat -A` or `sed -n 'N,Mp' | cat -A`, never with a Python
string literal that has to travel through a shell.** And never put an `rm` in
the same compound command as a check whose result decides whether the file was
good.

**What worked, every time, with no exceptions:** `Write` the payload to a file,
then splice it with a script whose own text contains **no backslash at all**
(`s.replace(anchor, open(f).read() + anchor, 1)`). The payload never meets a
shell; the script never needs an escape.

---

## ★★★ 2026-08-31 — `git checkout` AGAIN, and this time on the FIRST case the rule names

The `git checkout` half of this note has three prior instances and says, in
bold, with no scope on it: *"NEVER use `git checkout`, `git restore` or
`git stash` to undo anything while uncommitted work is in the tree."* The very
first sentence of that section is about undoing **a sabotage check**.

I ran `git checkout -- crates/pdfce-render/src/cmyk_buffer.rs` to undo a
sabotage check, and lost four uncommitted changes to that file: six corrected
guards, a runtime refusal, a 30-line rule exemption and a new test.

**Two things make this instance worth appending rather than just counting.**

**(1) It was in the SAME COMPOUND COMMAND as the sabotage.** I wrote
`python -c "...sabotage..." ; cargo test ; git checkout -- <file>` as one line,
so the restore was authored *before* the test had run and there was no moment
between reading the result and destroying the work. The rule fires at the point
of *deciding to restore*, and I had removed that point from the sequence.

⇒ **Never put the restore in the same command as the sabotage.** Copy the file
aside first (`cp x /tmp/x.bak`), run the sabotage, read the result, then restore
from the copy as a separate deliberate act. That is also what the note two
sections up already says about `rm` beside a check — same shape, same day.

**(2) The blast radius was invisible because it was PATH-SCOPED.** `git status`
afterwards showed a clean `crates/pdfce-render/`, three untracked agent files
and two unrelated modifications — which reads as a *tidy tree*, not as a loss.
Nothing was reported. I only knew because I had just written the code and could
grep for it.

⇒ **After any `git checkout`/`restore`, grep for a token you know you wrote.**
`git status` cannot tell you about work it has already discarded.

**The cheap structural fix, which I did not take and should have: commit
first.** All four changes were finished, tested and green before the sabotage.
A throwaway commit costs nothing and is reversible; a checkout is not. The rule
above already says this. Sabotage AFTER committing, restore with `git checkout`
freely, and the whole class disappears.

**A THIRD INSTANCE, 2026-09-02, and I first blamed the wrong thing.** A
patch script fed through the Bash tool as a `python - <<'EOF'` heredoc had
`\\` (an escaped backslash, to match a Rust line-continuation `\`) inside
its `old` anchor. (Writing THIS paragraph through a heredoc reproduced the
fault a third time — the `\\` above arrived as `\` and was fixed with Edit.) The transport delivered ONE backslash, so the anchor no
longer matched and `s.count(old)` returned 0. I wrote a memory saying the
cause was cp1252 decoding of the em dash on the same line; a direct test
showed non-ASCII arrives intact (`len("a — b") == 5`) and that a literal
`'\'` reaches Python as `''` (a SyntaxError on an unterminated string
proved it). Same class as the two instances above — a backslash through the
Bash tool is never literal — and the fix is the same: any patch script with
a backslash in it goes through the `Write` tool to a file, then
`python <file>`. Note the shape of the misdiagnosis: the wrong cause was the
most VISIBLE character on the failing line, not the one the class predicts.

**Recurred 2026-09-03 (fourth time), with a Rust byte-escape rather than a
path:** a Python heredoc patch containing `b'\n' | b'\r'` arrived in the
Rust source as literal LF/CR bytes; `grep` answered "Binary file matches"
and the file compiled only by accident. The tell is `cat -A` showing `^M`
or a bare `$` inside a quoted literal. **The rule is not "paths" — it is
ANY backslash, and Rust escapes (`\n`, `\r`, `\x0C`, `\0`, `\`) are the
ones a Rust project types most.** Write the patch with the Write tool to
`D:\Dev\temp\*.py` and run `python <file>`; the fix that day was exactly
that (`fix_trim_ws.py`).
