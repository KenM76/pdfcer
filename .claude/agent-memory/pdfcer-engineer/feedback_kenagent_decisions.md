---
name: kenagent-decision-protocol
description: Route non-trivial technical decisions through KenAgent (autonomous-builder agent); save Markdown rationale to docs/decisions/
metadata:
  type: feedback
---

For any non-trivial technical decision in pdfce (architecture choices, adopt-vs-build, dependency selection beyond routine picks), call the **autonomous-builder** agent ("KenAgent") via the Agent tool. Pass: the question, the project path (`D:\Dev\pdfce\`), and relevant context. It returns a decision with full reasoning in JSON and Markdown.

- Use the **JSON** to implement.
- Save the **Markdown** to `D:\Dev\pdfce\docs\decisions\` for project history (create the dir if absent; name files `NNN-short-slug.md`, sequential).

**Why:** Ken set this up 2026-07-30 as the standing decision-consultant mechanism so decisions carry his established preferences and leave an auditable rationale trail. Complements (does not replace) the existing rule that legal/license/copyleft calls are Ken's directly.

**How to apply:** Trigger on decisions the engineer agent file says to "raise with the user" or that would otherwise need judgment beyond ROADMAP/spec-RAG guidance — e.g. the oxidize-pdf adopt-vs-scratch gate, OCR engine binding, i18n timing. Trivial in-pattern engineering calls stay solo. Legal/license decisions still go to Ken himself per [[docs/LEGAL.md]] rules.
