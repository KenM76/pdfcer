#!/usr/bin/env bash
# check-bypass-paths.sh — every mutation of a document goes through
# `EditSession`, or is a NAMED exception with a stated reason.
#
# WHY THIS EXISTS
# ---------------
# `EditSession` is where undo, the operator-facing disclosures (rule 4), the
# certification gate (§12.8.4 Table 258) and the save-mode contract all live.
# A mutation that reaches the writer without passing through it gets NONE of
# them — and does so silently, producing a file that is wrong rather than a
# build that is red.
#
# The road around it is WIDE OPEN and nobody had noticed. `writer::DirtySet`
# exposes `empty`, `identity_reemission`, `replace`, `delete`, `patch_trailer`
# and — decisively — `set_staging`, all `pub`, alongside `save_incremental`
# and `save_full`. An external crate can today load a `Document`, build a
# `DirtySet` from nothing, author stream bytes into a staging buffer and write
# the file, with no undo entry, no disclosure and no certification check.
#
# Found while asking whether a future plugin system was foreclosed
# (decision 030). It is not — the opposite. The risk is that a plugin author
# finds `EditSession` closed, finds `DirtySet` open, and takes the open road,
# landing on a design where plugin edits bypass undo and disclosures BY
# DEFAULT rather than by decision.
#
# THIS GATE PAYS OFF EVEN IF NO PLUGIN SYSTEM IS EVER BUILT. Every bypass it
# catches is an edit with no undo and no disclosure — a defect on its own
# terms, today.
#
# WHY A GREP AND NOT A TYPE
# -------------------------
# Sealing the writer surface would be the real fix, and it is not free:
# `redact.rs` legitimately needs it (see below), and so do the packaging
# tools. Narrowing visibility to `pub(crate)` would break those and force a
# larger refactor than the risk currently justifies. This gate makes the
# bypass VISIBLE and deliberate at ~40 lines, which is the honest trade until
# someone wants the refactor.
#
# It also cannot be green-and-wrong in the way a subtler check could: to
# bypass `EditSession` you MUST name one of these symbols. There is no third
# way to reach the writer.
#
# THE SANCTIONED EXCEPTIONS, each with its reason
# -----------------------------------------------
# 1. `edit.rs` — this IS `EditSession`; it is the road, not a bypass.
#
# 2. `writer/` — the surface itself. A definition is not a use.
#
# 3. `redact.rs` — the ONE sanctioned traveller, and the reason is
#    substantive rather than historical. Redaction is R46's named exception
#    (ARCHITECTURE.md §5): it must FORCE a full rewrite, because an
#    incremental save would leave the redacted content recoverable in the
#    prior revision — the one outcome redaction exists to prevent. That is a
#    save-mode obligation `CommandKind` cannot express, so it goes around.
#    The cost is real and should not be forgotten: redaction's edits are
#    NON-UNDOABLE by construction.
#
# 4. `tools/` — the content-identity and parity harnesses read and re-emit
#    documents deliberately; they are instruments, not editing features.
#
# 5. `document.rs` — `Document::save_incremental` / `save_full` are thin
#    wrappers that ARE part of the writer surface; a definition is not a use.
#
# 6. `crates/*/tests/` — integration tests exercise the writer directly on
#    purpose, which is what a writer test is. `#[cfg(test)]` truncation cannot
#    reach them because an integration test file has no such marker.
#
# 7. `text_edit/addtext.rs` and `text_edit/edit.rs` — the ONE-SHOT standalone
#    APIs (`add_text(doc, req) -> bytes`, `edit_text(doc, req, opts)`), which
#    the CLI calls directly. They predate `EditSession` and are its siblings,
#    not its evaders: a one-shot transformation of a `Document` that is not in
#    an edit session has no undo stack to join and nothing to disclose to a
#    later command. `add_text` honours the certification gate
#    (`refuse_if_certification_forbids`); both refuse an encrypted document.
#
#    VERIFIED WHILE ADDING THIS EXCEPTION, and worth stating because it looks
#    like a bypass finding and is not: `edit_text` does NOT check
#    certification — but neither does `EditSession::edit_text`. The absence is
#    consistent across both paths and is an add-text/edit-text inconsistency,
#    not a session/standalone one. Filed separately; do not "fix" it here.
#
# 8. `pdfcer`'s `round-trip` subcommand — `DirtySet::empty()` is identity
#    re-emission. It mutates NOTHING; it exists to prove the writer reproduces
#    a file byte-for-byte. An instrument, like `tools/`.
#
# 9. `bypass-exempt: <reason>` within eight lines of the hit. Borrowed from
#    `check-ui-strings.sh`'s `ui-text-exempt` idiom, and preferred over
#    widening the file list above: an exemption written AT the call site is
#    read by whoever is about to add a second one, whereas a file name in this
#    script is not. This is how exception 8 is actually enforced — see the
#    window comment below for why it is a window and not a same-line marker.
#
# 10. Anything from `#[cfg(test)]` to end of file. A test constructing a
#    `DirtySet` directly is testing the writer, which is what a writer test
#    should do. Same truncation rule (and same limit) as
#    `check-ui-strings.sh`: code placed AFTER the test module is invisible
#    here.
#
# EXIT CODES: 0 clean, 1 violations found.

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

# The five symbols that reach the writer without EditSession. `DirtySet` alone
# would over-match (the type name appears in doc comments across the crate),
# so this looks for CONSTRUCTION and MUTATION calls plus the save entry
# points.
PATTERN='DirtySet::(empty|identity_reemission)|\.set_staging\(|\.patch_trailer\(|writer::save_(incremental|full)\(|save_(incremental|full)\('

violations=0
report=""

while IFS= read -r file; do
  case "$file" in
    */pdfcer-core/src/edit.rs) continue ;;
    */pdfcer-core/src/writer/*) continue ;;
    */pdfcer-core/src/redact.rs) continue ;;
    */pdfcer-core/src/document.rs) continue ;;
    */tests/*) continue ;;
    */pdfcer-core/src/text_edit/addtext.rs) continue ;;
    */pdfcer-core/src/text_edit/edit.rs) continue ;;
    ./tools/*) continue ;;
  esac

  # Truncate at the first `#[cfg(test)]` — test code may touch the writer.
  cut=$(grep -n '#\[cfg(test)\]' "$file" | head -1 | cut -d: -f1)
  if [ -n "$cut" ]; then
    body=$(head -n "$((cut - 1))" "$file")
  else
    body=$(cat "$file")
  fi

  # Exceptions are matched near the CALL SITE rather than by file, so the rest
  # of `main.rs` stays covered — the CLI is exactly where a future bypass would
  # land, and excluding the whole file to spare one instrument would give up
  # the coverage that matters most.
  raw=$(printf '%s\n' "$body" \
    | grep -nE "$PATTERN" \
    | grep -v '^[0-9]*: *//' \
    || true)

  # An exemption is honoured if `bypass-exempt:` appears WITHIN EIGHT LINES
  # of the hit, either side.
  #
  # It started as a same-line marker, which `cargo fmt` promptly broke: given
  # a multi-line call, rustfmt moves a trailing comment from the operator's
  # line down INSIDE the argument list. The gate went green, then red on the
  # next `fmt`, with no source change in between — the worst kind of gate.
  # A window is what survives a formatter that is allowed to move comments.
  #
  # EIGHT, not three, and the number was found by the gate refusing to go
  # green rather than chosen: one `match` with three arms already spans more
  # than three lines, so a tighter window forces a marker per arm — which
  # rustfmt is then free to relocate again. Eight covers a small block.
  #
  # STATED LIMIT: this is proximity, not scope. A marker CAN cover a genuine
  # bypass that happens to sit within eight lines of an exempted one. If that
  # ever matters, the fix is to exempt by enclosing function, which needs more
  # than grep. Until then a reviewer reading the marker sees both calls.
  hits=""
  while IFS= read -r h; do
    [ -z "$h" ] && continue
    n=${h%%:*}
    lo=$((n > 8 ? n - 8 : 1))
    hi=$((n + 8))
    if printf '%s\n' "$body" | sed -n "${lo},${hi}p" | grep -q 'bypass-exempt:'; then
      continue
    fi
    hits="${hits}${h}"$'\n'
  done <<< "$raw"
  hits=$(printf '%s' "$hits")
  if [ -n "$hits" ]; then
    while IFS= read -r h; do
      report="${report}${file}:${h}"$'\n'
      violations=$((violations + 1))
    done <<< "$hits"
  fi
done < <(find ./crates -name '*.rs' -type f)

if [ "$violations" -gt 0 ]; then
  printf '%s' "$report"
  echo
  echo "error: $violations document-mutation path(s) bypassing EditSession."
  echo "A mutation that skips EditSession gets no undo entry, no rule-4"
  echo "disclosure and no certification check — silently, in the output file."
  echo "Route it through EditSession, or add it to this script's exception"
  echo "list WITH ITS REASON (see the header; redaction's is the model)."
  exit 1
fi

echo "bypass-paths: clean — every document mutation goes through EditSession"
exit 0
