"""Generate encrypted PDFs in every standard-handler mode, as decryption
falsifiers.

Source document is synthetic and self-authored (project rule 7). The
encryption is produced by pypdf — an INDEPENDENT implementation, which is the
whole point: pdfcer's decryption will be written from ISO 32000-1 §7.6 and
then checked against files it did not produce. Agreement then means two
independent readings of the same clause agree, which is evidence; agreement
with its own output would mean nothing.

Caveat recorded up front: this cuts one way only. For R2/R3/R4 and AES-128,
ISO 32000-1 fully specifies the algorithms, so pypdf's files are a genuine
cross-check of a spec-derived implementation. For **R6 (AES-256)** the
algorithm is NOT sourced — deriving it from pypdf and then testing against
pypdf would be circular, and these files are therefore refusal fixtures, not
acceptance fixtures.

★ THE SOURCE DOCUMENT IS DEFAULTED, NOT REQUIRED — and that is a fix, not a
convenience. The first version of this script took the source as a mandatory
argument, and the six fixtures committed on 2026-08-11 were generated from a
document that was NEVER COMMITTED: a four-field calculation form living in a
session temp folder. Running the script afterwards therefore produced a
DIFFERENT corpus, silently, and the discrepancy only surfaced because someone
tried to add a seventh fixture and noticed the other six changing size.

That is exactly the failure this script's own PROVENANCE note warned about —
"a fixture whose construction nobody can repeat is a fixture nobody can
extend" — arriving one week after the note was written. The default below
points at a **committed** fixture, so the corpus is reproducible from a clean
checkout by anyone, with no argument to get wrong.
"""
import os
import sys
from pypdf import PdfWriter, PdfReader

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_SRC = os.path.join(HERE, '..', 'fixtures', 'synthetic', 'forms', 'demo-form.pdf')
DEFAULT_OUT = os.path.join(HERE, '..', 'fixtures', 'synthetic', 'encryption')

src = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_SRC
outdir = sys.argv[2] if len(sys.argv) > 2 else DEFAULT_OUT
print(f'source:  {os.path.normpath(src)}')
print(f'outdir:  {os.path.normpath(outdir)}')

MODES = [
    ('rc4-40', 'RC4_40'),
    ('rc4-128', 'RC4_128'),
    ('aes-128', 'AES_128'),
    ('aes-256-r5', 'AES_256_R5'),
    ('aes-256-r6', 'AES_256'),
]

USER = 'userpw'
OWNER = 'ownerpw'

for name, algo in MODES:
    w = PdfWriter(clone_from=src)
    w.encrypt(user_password=USER, owner_password=OWNER, algorithm=algo)
    path = f'{outdir}/enc-{name}.pdf'
    with open(path, 'wb') as f:
        w.write(f)
    # Read the /Encrypt dictionary back so the fixture's own parameters are
    # visible without a hex editor.
    r = PdfReader(path)
    enc = r.trailer['/Encrypt'].get_object()
    print(f'{name:12} V={enc.get("/V")} R={enc.get("/R")} '
          f'Length={enc.get("/Length")} P={enc.get("/P")} '
          f'CFM={enc.get("/CF", {}).get("/StdCF", {}).get("/CFM", "-")}')

# And the EMPTY user password — the case §7.6.3.1 says a reader shall try
# silently before prompting, which is why permissions-only PDFs open
# everywhere with no dialog. It is the single most operator-visible behaviour
# in clause 7.6, so it gets a fixture in EVERY cipher rather than one.
#
# ★ Originally there was only the AES-128 file below. That was a hole: pdfcer
# implements ciphers one increment at a time, and while AES is refused, an
# AES-only empty-password fixture means the empty-password PATH ITSELF is
# never exercised end-to-end — the file is rejected on cipher grounds before
# authentication is ever reached. A fixture that cannot fail for the reason
# you care about is not covering that reason.
#
# ★ AND IT WAS A HOLE AGAIN, ONE CIPHER LATER. The comment above promises a
# fixture in EVERY cipher; when AES-256 at /R 5 was implemented (increment 3)
# there were still only two, so the /R 5 branch of the silent empty-password
# attempt was implemented, believed and never once executed. Exactly the shape
# the paragraph above describes, in the code that describes it. A promise in a
# comment is not a fixture — `enc-emptyuser-aes-256-r5.pdf` is.
for name, algo in [
    ('emptyuser-rc4-128', 'RC4_128'),
    ('emptyuser', 'AES_128'),
    ('emptyuser-aes-256-r5', 'AES_256_R5'),
]:
    w = PdfWriter(clone_from=src)
    w.encrypt(user_password='', owner_password=OWNER, algorithm=algo)
    with open(f'{outdir}/enc-{name}.pdf', 'wb') as f:
        w.write(f)
    print(f'{name:18} {algo}, user password is the empty string')
