# Encrypted-document fixtures

**Synthetic and self-generated** (project rule 7 / `LEGAL.md` §5). The
plaintext source is `fixtures/synthetic/forms/demo-form.pdf` — pdfcer's own
synthetic form fixture, **committed**, so the corpus is reproducible from a
clean checkout. The encryption was applied by **pypdf 6.7.0**, chosen
deliberately as an *independent* implementation.

Passwords: user `userpw`, owner `ownerpw`, except the two `emptyuser` files
whose user password is the **empty string** — the case §7.6.3.1 says a reader
*shall* try silently before prompting, and the reason permissions-only PDFs
open everywhere with no dialog.

| File | `/V` | `/R` | `/Length` | `/CFM` | pdfcer reads it? |
|---|---|---|---|---|---|
| `enc-rc4-40.pdf` | 1 | 2 | 40 | — | **yes** |
| `enc-rc4-128.pdf` | 2 | 3 | 128 | — | **yes** |
| `enc-emptyuser-rc4-128.pdf` | 2 | 3 | 128 | — | **yes**, with no password |
| `enc-aes-128.pdf` | 4 | 4 | 128 | `/AESV2` | **yes** |
| `enc-aes-256-r5.pdf` | 5 | 5 | 256 | `/AESV3` | **yes** |
| `enc-aes-256-r6.pdf` | 5 | 6 | 256 | `/AESV3` | refused as **unsourced** |
| `enc-emptyuser.pdf` | 4 | 4 | 128 | `/AESV2` | **yes**, with no password |
| `enc-emptyuser-aes-256-r5.pdf` | 5 | 5 | 256 | `/AESV3` | **yes**, with no password |

Note `enc-aes-256-r5.pdf`'s `/Length 256`. `/AESV3` fixes the key at 256 bits,
so the entry carries no information and pdfcer does not read it — ISO 32000-2's
Table 25 erratum says a *standard* handler should write **32** while
2.0-as-printed said **256**, and both appear in the wild (**W18**, ambiguity
**A11**). pypdf followed the printed text.

## ★ What these can and cannot prove

**They cut one way only, and the distinction is the whole point.**

For **`/R` 2, 3 and 4**, ISO 32000-1 §7.6 fully specifies the algorithms.
pdfcer's decryption was written from the clause, then checked against files it
did not produce — so agreement means two independent readings of the same
specification agree. That is evidence, and as of 2026-08-11 it is *collected*
evidence: `crates/pdfcer-core/tests/encryption_rc4.rs` opens both RC4 fixtures
with both the user and the owner password.

For **`/R` 6**, the algorithm (2.B) is **not sourced**: ISO 32000-2 is
paywalled past step (a). Deriving it from another implementation and then
testing against that implementation's output would be circular — the test
could not fail. `enc-aes-256-r6.pdf` is therefore a **refusal fixture**:
pdfcer must decline it *by name*, distinguished from `/R` 5, and the test
asserts the refusal rather than a decrypt.

`enc-aes-256-r5.pdf` sits between the two. `/R` 5 is a deprecated Adobe
extension, paraphrased in the corpus rather than sourced from ISO, and PDF
2.0 deprecates handler revisions 1–5 outright. Reading it is still required —
Acrobat wrote such files between 2008 and 2011, and deprecation does not
un-write them. **It became an acceptance fixture in encryption increment 3**
(2026-08-11); the deprecation is a bar on *writing* `/R` 5, never on reading it.

## ★ What this corpus still cannot fail on

**Object streams.** Every file here derives from `demo-form.pdf`, a PDF 1.3
document with a classic cross-reference table and **zero object streams**, and
pypdf *flattens* object streams when it clones (measured: a 7-`ObjStm` source
came out with 0). So the corpus cannot be extended to cover the commonest
real-world shape by changing its source document — and that shape is exactly
where AES is most dangerous, because an object-stream container is itself a
stream whose span shortens on decryption and is then re-parsed.

Two gitignored external files cover it instead, each skipping loudly when
absent (`docs/LEGAL.md` §5 — the external corpora are cloned, never vendored):

| File | Covers | Password |
|---|---|---|
| `fixtures/external/pdfium/testing/resources/encrypted.pdf` | `/V` 4, `/R` 4, `/AESV2`, 5 object streams, 2 xref streams | `1234` |
| `fixtures/external/qpdf/qpdf/qtest/qpdf/c-r5-in.pdf` | `/V` 5, `/R` 5, `/AESV3`, 1 object stream | `user3` / `owner3` |

The qpdf file earns a second mention: its test suite **publishes the expected
file encryption key** (`35ea16a4…a020`, asserted by
`qpdf --check --show-encryption-key` and accepted by
`--password-is-hex-key`). That value is copied into `crypto::r5`'s unit tests
as an **unconditional** cross-implementation vector, so the strongest half of
the `/R` 5 evidence does not depend on a corpus that may not be present. A
skipping test is not coverage; a test vector is.

**Non-ASCII passwords.** Every password here is ASCII, where `/R` 5's SASLprep
step is the identity function. No fixture exercises a password that
normalisation would change, and none can be built without implementing
SASLprep first — which is why pdfcer *discloses* the gap on a failed non-ASCII
authentication rather than claiming to have handled it.

## ★ Why there are three `emptyuser` files

There was one, and it was AES-128, and that was a hole.

The empty-user-password path is the most operator-visible behaviour in clause
7.6 — it is what makes a permissions-only PDF open with no dialog anywhere.
But pdfcer implements ciphers one increment at a time, and while AES is
refused, an AES-only empty-password fixture is rejected **on cipher grounds
before authentication is ever reached**. The path was implemented, believed,
and never once executed end-to-end.

`enc-emptyuser-rc4-128.pdf` exists so it is. The AES file stays, because it
becomes the same test for the next increment.

**★ And it happened again, one cipher later.** Increment 3 implemented AES-256
at `/R` 5 — a genuinely different authentication path, Algorithm 3.11 against
`/U[0..32]` rather than Algorithm 6 against `/U[0..16]` — and there were still
only two empty-password fixtures. So the `/R` 5 branch of the silent attempt
was in exactly the state the paragraphs above describe: implemented, believed,
never executed. `enc-emptyuser-aes-256-r5.pdf` closes it. Note where the miss
happened: the generator script's own comment already promised "a fixture in
EVERY cipher rather than one", and the promise was not kept by the change that
added the cipher. A promise in a comment is not a fixture.

The general form is worth keeping: *a fixture that cannot fail for the reason
you care about is not covering that reason.*

## ★ This corpus is NOT byte-reproducible, and one test depends on that

Re-running the generator produces a **different** `/O`, `/U`, `/OE`, `/UE` and
`/Perms` for the same document every time — encryption generates fresh salts
and a fresh file encryption key, by design. Only the *shape* is reproducible.

`crypto::r5`'s unit tests embed `enc-aes-256-r5.pdf`'s actual bytes as
constants, so each `/R` 5 algorithm can be exercised in isolation and say
*which* one broke. That coupling is invisible from both sides, so it is
asserted:
`crates/pdfcer-core/tests/encryption.rs::the_r5_fixture_still_matches_the_unit_test_constants`
goes red if the fixture is regenerated. **If it does, copy the new bytes into
those constants — do not weaken the test.** Without it a regeneration would
leave the unit tests quietly passing against a file that no longer exists.

## Regenerating

```
python tools/gen-encryption-fixtures.py
```

No arguments needed — source and output default to committed paths.

**That defaulting is a fix, not a convenience.** The first six fixtures were
generated from a document that was never committed (a four-field calculation
form in a session temp folder), so re-running the script produced a *different
corpus*, silently. It surfaced only when someone tried to add a seventh
fixture and noticed the other six changing size — one week after this file's
own closing sentence warned about exactly that. The whole corpus was
regenerated on 2026-08-11 from the committed source, so the promise below is
now true rather than aspirational.

Kept because a fixture whose construction nobody can repeat is a fixture
nobody can extend.
