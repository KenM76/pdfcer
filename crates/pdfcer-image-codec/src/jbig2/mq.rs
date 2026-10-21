//! The MQ arithmetic encoder (ITU-T T.88 Annex E, Figures E.3-E.12).
//!
//! Spec source: `filters/filter__jbig2_generic_encoder.md` §4-§5 in the
//! PDF-spec RAG. Every context starts at index 0 with MPS 0 (E.2.8). The
//! byte before the first output byte (E.2.8's `BPST - 1`) is a dummy 0x00
//! that is dropped at the end; the first `BYTEOUT` cannot carry into it.
//! No E.2.10 trimming, which is optional.

/// One row of Table E.1.
#[derive(Clone, Copy)]
struct State {
    qe: u32,
    nmps: u8,
    nlps: u8,
    switch: bool,
}

/// Table E.1, transcribed by script from the RAG.
const TABLE: [State; 47] = [
    State {
        qe: 0x5601,
        nmps: 1,
        nlps: 1,
        switch: true,
    },
    State {
        qe: 0x3401,
        nmps: 2,
        nlps: 6,
        switch: false,
    },
    State {
        qe: 0x1801,
        nmps: 3,
        nlps: 9,
        switch: false,
    },
    State {
        qe: 0x0AC1,
        nmps: 4,
        nlps: 12,
        switch: false,
    },
    State {
        qe: 0x0521,
        nmps: 5,
        nlps: 29,
        switch: false,
    },
    State {
        qe: 0x0221,
        nmps: 38,
        nlps: 33,
        switch: false,
    },
    State {
        qe: 0x5601,
        nmps: 7,
        nlps: 6,
        switch: true,
    },
    State {
        qe: 0x5401,
        nmps: 8,
        nlps: 14,
        switch: false,
    },
    State {
        qe: 0x4801,
        nmps: 9,
        nlps: 14,
        switch: false,
    },
    State {
        qe: 0x3801,
        nmps: 10,
        nlps: 14,
        switch: false,
    },
    State {
        qe: 0x3001,
        nmps: 11,
        nlps: 17,
        switch: false,
    },
    State {
        qe: 0x2401,
        nmps: 12,
        nlps: 18,
        switch: false,
    },
    State {
        qe: 0x1C01,
        nmps: 13,
        nlps: 20,
        switch: false,
    },
    State {
        qe: 0x1601,
        nmps: 29,
        nlps: 21,
        switch: false,
    },
    State {
        qe: 0x5601,
        nmps: 15,
        nlps: 14,
        switch: true,
    },
    State {
        qe: 0x5401,
        nmps: 16,
        nlps: 14,
        switch: false,
    },
    State {
        qe: 0x5101,
        nmps: 17,
        nlps: 15,
        switch: false,
    },
    State {
        qe: 0x4801,
        nmps: 18,
        nlps: 16,
        switch: false,
    },
    State {
        qe: 0x3801,
        nmps: 19,
        nlps: 17,
        switch: false,
    },
    State {
        qe: 0x3401,
        nmps: 20,
        nlps: 18,
        switch: false,
    },
    State {
        qe: 0x3001,
        nmps: 21,
        nlps: 19,
        switch: false,
    },
    State {
        qe: 0x2801,
        nmps: 22,
        nlps: 19,
        switch: false,
    },
    State {
        qe: 0x2401,
        nmps: 23,
        nlps: 20,
        switch: false,
    },
    State {
        qe: 0x2201,
        nmps: 24,
        nlps: 21,
        switch: false,
    },
    State {
        qe: 0x1C01,
        nmps: 25,
        nlps: 22,
        switch: false,
    },
    State {
        qe: 0x1801,
        nmps: 26,
        nlps: 23,
        switch: false,
    },
    State {
        qe: 0x1601,
        nmps: 27,
        nlps: 24,
        switch: false,
    },
    State {
        qe: 0x1401,
        nmps: 28,
        nlps: 25,
        switch: false,
    },
    State {
        qe: 0x1201,
        nmps: 29,
        nlps: 26,
        switch: false,
    },
    State {
        qe: 0x1101,
        nmps: 30,
        nlps: 27,
        switch: false,
    },
    State {
        qe: 0x0AC1,
        nmps: 31,
        nlps: 28,
        switch: false,
    },
    State {
        qe: 0x09C1,
        nmps: 32,
        nlps: 29,
        switch: false,
    },
    State {
        qe: 0x08A1,
        nmps: 33,
        nlps: 30,
        switch: false,
    },
    State {
        qe: 0x0521,
        nmps: 34,
        nlps: 31,
        switch: false,
    },
    State {
        qe: 0x0441,
        nmps: 35,
        nlps: 32,
        switch: false,
    },
    State {
        qe: 0x02A1,
        nmps: 36,
        nlps: 33,
        switch: false,
    },
    State {
        qe: 0x0221,
        nmps: 37,
        nlps: 34,
        switch: false,
    },
    State {
        qe: 0x0141,
        nmps: 38,
        nlps: 35,
        switch: false,
    },
    State {
        qe: 0x0111,
        nmps: 39,
        nlps: 36,
        switch: false,
    },
    State {
        qe: 0x0085,
        nmps: 40,
        nlps: 37,
        switch: false,
    },
    State {
        qe: 0x0049,
        nmps: 41,
        nlps: 38,
        switch: false,
    },
    State {
        qe: 0x0025,
        nmps: 42,
        nlps: 39,
        switch: false,
    },
    State {
        qe: 0x0015,
        nmps: 43,
        nlps: 40,
        switch: false,
    },
    State {
        qe: 0x0009,
        nmps: 44,
        nlps: 41,
        switch: false,
    },
    State {
        qe: 0x0005,
        nmps: 45,
        nlps: 42,
        switch: false,
    },
    State {
        qe: 0x0001,
        nmps: 45,
        nlps: 43,
        switch: false,
    },
    State {
        qe: 0x5601,
        nmps: 46,
        nlps: 46,
        switch: false,
    },
];

/// A context's adaptive state: its Table E.1 index and its MPS.
#[derive(Clone, Copy, Default)]
struct Context {
    index: u8,
    mps: u8,
}

/// The encoder registers of E.2.1, with `B` as the last output byte.
pub(super) struct MqEncoder {
    a: u32,
    c: u32,
    ct: u32,
    out: Vec<u8>,
    contexts: Vec<Context>,
}

impl MqEncoder {
    /// INITENC (Figure E.10) over `contexts` adaptive contexts.
    pub(super) fn new(contexts: usize) -> Self {
        Self {
            a: 0x8000,
            c: 0,
            ct: 12,
            out: vec![0],
            contexts: vec![Context::default(); contexts],
        }
    }

    /// ENCODE (Figure E.3): codes `bit` in context `cx`. A context past
    /// the end is a programming error and codes nothing.
    pub(super) fn encode(&mut self, cx: usize, bit: bool) {
        let Some(&context) = self.contexts.get(cx) else {
            return;
        };
        let state = TABLE
            .get(usize::from(context.index))
            .copied()
            .unwrap_or(TABLE[46]);
        let next = if u8::from(bit) == context.mps {
            self.code_mps(state, context)
        } else {
            self.code_lps(state, context)
        };
        if let Some(slot) = self.contexts.get_mut(cx) {
            *slot = next;
        }
    }

    /// CODEMPS (Figure E.7), with its conditional exchange.
    fn code_mps(&mut self, state: State, mut context: Context) -> Context {
        self.a -= state.qe;
        if self.a & 0x8000 == 0 {
            if self.a < state.qe {
                self.a = state.qe;
            } else {
                self.c += state.qe;
            }
            context.index = state.nmps;
            self.renorm();
        } else {
            self.c += state.qe;
        }
        context
    }

    /// CODELPS (Figure E.6), with its conditional exchange.
    fn code_lps(&mut self, state: State, mut context: Context) -> Context {
        self.a -= state.qe;
        if self.a < state.qe {
            self.c += state.qe;
        } else {
            self.a = state.qe;
        }
        if state.switch {
            context.mps = 1 - context.mps;
        }
        context.index = state.nlps;
        self.renorm();
        context
    }

    /// RENORME (Figure E.8).
    fn renorm(&mut self) {
        loop {
            self.a <<= 1;
            self.c <<= 1;
            self.ct -= 1;
            if self.ct == 0 {
                self.byte_out();
            }
            if self.a & 0x8000 != 0 {
                break;
            }
        }
    }

    /// BYTEOUT (Figure E.9). A carry adds to the last byte only (E.2.7).
    fn byte_out(&mut self) {
        let Some(last) = self.out.last_mut() else {
            return;
        };
        if *last == 0xFF {
            self.stuff();
        } else if self.c < 0x800_0000 {
            self.normal();
        } else {
            *last += 1;
            if *last == 0xFF {
                self.c &= 0x7FF_FFFF;
                self.stuff();
            } else {
                self.normal();
            }
        }
    }

    /// The masked shift: bit 27 of C is the carry already added to `B`.
    fn normal(&mut self) {
        self.out.push(((self.c >> 19) & 0xFF) as u8);
        self.c &= 0x7_FFFF;
        self.ct = 8;
    }

    fn stuff(&mut self) {
        self.out.push(((self.c >> 20) & 0xFF) as u8);
        self.c &= 0xF_FFFF;
        self.ct = 7;
    }

    /// FLUSH (Figure E.11) and SETBITS (Figure E.12): the coded bytes,
    /// ending with the `0xFF 0xAC` marker, without the dummy byte.
    pub(super) fn finish(mut self) -> Vec<u8> {
        let tempc = self.c + self.a;
        self.c |= 0xFFFF;
        if self.c >= tempc {
            self.c -= 0x8000;
        }
        self.c <<= self.ct;
        self.byte_out();
        self.c <<= self.ct;
        self.byte_out();
        if self.out.last() != Some(&0xFF) {
            self.out.push(0xFF);
        }
        self.out.push(0xAC);
        self.out.split_off(1)
    }
}
