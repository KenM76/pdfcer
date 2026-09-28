//! Rewrites an ONNX model exported below opset 13 into the node forms `rten`
//! reads.
//!
//! `rten` parses every operator by its opset-13+ definition and ignores the
//! model's `opset_import`. So an older export either fails to load (an
//! attribute the operator no longer has) or, worse, loads and computes
//! something else: before opset 13, `Softmax(axis=a)` normalised over every
//! dimension from `a` on, flattened, not over `a` alone. PaddlePaddle's own
//! PP-OCRv5 recogniser exports are opset 7 and hit all of these.
//!
//! The rewrites are the ones `onnx.version_converter` applies, weights
//! untouched:
//!
//! | Operator | Below opset | Rewrite |
//! |---|---|---|
//! | `BatchNormalization` | 9 | drop `spatial` (only `spatial = 1` has a later form; `0` is refused) |
//! | `Slice` | 10 | `starts`/`ends`/`axes` attributes become `Constant` inputs |
//! | `Softmax`, `LogSoftmax`, `Hardmax` | 13 | `Shape` → `Flatten(axis)` → op(`axis = 1`) → `Reshape` to the input's shape |
//!
//! Every other node keeps its content (a node's fields may be re-ordered on
//! the wire); control-flow subgraphs are rewritten recursively. A model with
//! nothing to rewrite, or at opset 13 or later, is returned byte-identical.
//! Operators whose older forms
//! `rten` already reads (`Squeeze`/`Unsqueeze` with an `axes` attribute) are
//! left alone.
//!
//! The parser works on the protobuf wire format directly (the schema is
//! `onnx.proto`, field numbers cited at each constant). It is untrusted input:
//! every length is bounds-checked and subgraph recursion is capped at
//! [`MAX_GRAPH_DEPTH`].

/// Deepest subgraph nesting rewritten; deeper is refused.
pub const MAX_GRAPH_DEPTH: usize = 16;

/// The opset whose operator forms `rten` implements.
pub const TARGET_OPSET: i64 = 13;

// ModelProto
const MODEL_GRAPH: u32 = 7;
const MODEL_OPSET_IMPORT: u32 = 8;
// OperatorSetIdProto
const OPSET_DOMAIN: u32 = 1;
const OPSET_VERSION: u32 = 2;
// GraphProto
const GRAPH_NODE: u32 = 1;
// NodeProto
const NODE_INPUT: u32 = 1;
const NODE_OUTPUT: u32 = 2;
const NODE_OP_TYPE: u32 = 4;
const NODE_ATTRIBUTE: u32 = 5;
const NODE_DOMAIN: u32 = 7;
// AttributeProto
const ATTR_NAME: u32 = 1;
const ATTR_I: u32 = 3;
const ATTR_T: u32 = 5;
const ATTR_G: u32 = 6;
const ATTR_INTS: u32 = 8;
const ATTR_GRAPHS: u32 = 11;
const ATTR_TYPE: u32 = 20;
const ATTR_TYPE_INT: u64 = 2;
const ATTR_TYPE_TENSOR: u64 = 4;
// TensorProto
const TENSOR_DIMS: u32 = 1;
const TENSOR_DATA_TYPE: u32 = 2;
const TENSOR_RAW_DATA: u32 = 9;
const TENSOR_INT64: u64 = 7;

const WIRE_VARINT: u8 = 0;
const WIRE_FIXED64: u8 = 1;
const WIRE_LEN: u8 = 2;
const WIRE_FIXED32: u8 = 5;

/// A model after [`upgrade`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Upgrade {
    /// The model bytes to load: the input itself when nothing was rewritten.
    pub bytes: Vec<u8>,
    /// The default-domain opset the model declared, if it declared one.
    pub opset: Option<i64>,
    /// How many nodes were rewritten (zero when the model was returned
    /// unchanged).
    pub rewritten: usize,
}

/// Why a model could not be upgraded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum UpgradeError {
    /// The bytes are not a well-formed ONNX protobuf.
    #[error("not a well-formed ONNX model: {0}")]
    Malformed(&'static str),
    /// A node has no opset-13 equivalent pdfcer can write.
    #[error("cannot upgrade this opset-{opset} model to opset 13: {reason}")]
    Unsupported {
        /// The model's declared default-domain opset.
        opset: i64,
        /// Which node and why.
        reason: String,
    },
}

/// Upgrade `model` to the opset-13 forms of the operators in the module
/// table.
///
/// # Errors
///
/// [`UpgradeError::Malformed`] for bytes that are not a readable
/// `ModelProto`; [`UpgradeError::Unsupported`] for a node with no later form
/// (`BatchNormalization` with `spatial = 0`, a `Slice` without `starts` or
/// `ends`) or subgraphs nested deeper than [`MAX_GRAPH_DEPTH`].
pub fn upgrade(model: Vec<u8>) -> Result<Upgrade, UpgradeError> {
    let mut opset = None;
    let mut graphs = 0usize;
    for field in Fields::new(&model) {
        let field = field?;
        match (field.num, field.value) {
            (MODEL_OPSET_IMPORT, Value::Len(body)) => {
                if let Some(v) = default_domain_version(body)? {
                    opset = Some(v);
                }
            }
            (MODEL_GRAPH, Value::Len(_)) => graphs += 1,
            _ => {}
        }
    }
    let Some(version) = opset.filter(|&v| v < TARGET_OPSET) else {
        return Ok(Upgrade {
            bytes: model,
            opset,
            rewritten: 0,
        });
    };
    if graphs != 1 {
        return Err(UpgradeError::Malformed(
            "a model must hold exactly one graph",
        ));
    }
    let mut rw = Rewriter {
        opset: version,
        rewritten: 0,
        next_id: 0,
    };
    let mut out = Vec::with_capacity(model.len() + 4096);
    for field in Fields::new(&model) {
        let field = field?;
        match (field.num, field.value) {
            (MODEL_GRAPH, Value::Len(body)) => {
                let graph = rw.graph(body, 0)?;
                put_len(&mut out, MODEL_GRAPH, &graph);
            }
            _ => out.extend_from_slice(field.raw),
        }
    }
    if rw.rewritten == 0 {
        return Ok(Upgrade {
            bytes: model,
            opset,
            rewritten: 0,
        });
    }
    Ok(Upgrade {
        bytes: out,
        opset,
        rewritten: rw.rewritten,
    })
}

/// The version of an `OperatorSetIdProto` whose domain is the default one
/// (`""` or `ai.onnx`), or `None` for another domain.
fn default_domain_version(body: &[u8]) -> Result<Option<i64>, UpgradeError> {
    let mut domain: &[u8] = b"";
    let mut version = 0i64;
    for field in Fields::new(body) {
        let field = field?;
        match (field.num, field.value) {
            (OPSET_DOMAIN, Value::Len(d)) => domain = d,
            #[allow(clippy::cast_possible_wrap)] // int64 on the wire is two's complement
            (OPSET_VERSION, Value::Varint(v)) => version = v as i64,
            _ => {}
        }
    }
    Ok((domain.is_empty() || domain == b"ai.onnx").then_some(version))
}

struct Rewriter {
    opset: i64,
    rewritten: usize,
    next_id: usize,
}

impl Rewriter {
    fn unsupported(&self, reason: String) -> UpgradeError {
        UpgradeError::Unsupported {
            opset: self.opset,
            reason,
        }
    }

    fn graph(&mut self, body: &[u8], depth: usize) -> Result<Vec<u8>, UpgradeError> {
        if depth >= MAX_GRAPH_DEPTH {
            return Err(self.unsupported(format!(
                "subgraphs nest deeper than {MAX_GRAPH_DEPTH} levels"
            )));
        }
        let mut out = Vec::with_capacity(body.len() + 1024);
        for field in Fields::new(body) {
            let field = field?;
            match (field.num, field.value) {
                (GRAPH_NODE, Value::Len(node)) => self.node(node, depth, &mut out)?,
                _ => out.extend_from_slice(field.raw),
            }
        }
        Ok(out)
    }

    /// Append `node`, rewritten if its operator is in the table, to `out` as
    /// one or more `GraphProto.node` fields.
    fn node(&mut self, body: &[u8], depth: usize, out: &mut Vec<u8>) -> Result<(), UpgradeError> {
        let mut n = Node::parse(body)?;
        if n.attrs.iter().any(|a| a.has_graph) {
            n.attrs = n
                .attrs
                .into_iter()
                .map(|a| self.attr_subgraphs(a, depth))
                .collect::<Result<_, _>>()?;
            // The node itself changed only if a subgraph did; re-encode.
        }
        let default_domain = n.domain.is_empty() || n.domain == b"ai.onnx";
        let op = n.op_type;
        let emitted: Vec<Vec<u8>> = if !default_domain {
            vec![n.encode()]
        } else if op == b"BatchNormalization" && self.opset < 9 {
            self.batch_norm(n)?
        } else if op == b"Slice" && self.opset < 10 {
            self.slice(n)?
        } else if matches!(op, b"Softmax" | b"LogSoftmax" | b"Hardmax") {
            self.softmax(n)?
        } else {
            vec![n.encode()]
        };
        for node in emitted {
            put_len(out, GRAPH_NODE, &node);
        }
        Ok(())
    }

    fn attr_subgraphs<'a>(&mut self, a: Attr<'a>, depth: usize) -> Result<Attr<'a>, UpgradeError> {
        let mut out = Vec::with_capacity(a.raw.len());
        for field in Fields::new(a.raw) {
            let field = field?;
            match (field.num, field.value) {
                (ATTR_G | ATTR_GRAPHS, Value::Len(g)) => {
                    let g = self.graph(g, depth + 1)?;
                    put_len(&mut out, field.num, &g);
                }
                _ => out.extend_from_slice(field.raw),
            }
        }
        Ok(Attr {
            owned: Some(out),
            ..a
        })
    }

    fn batch_norm(&mut self, mut n: Node<'_>) -> Result<Vec<Vec<u8>>, UpgradeError> {
        let Some(at) = n.attrs.iter().position(|a| a.name == b"spatial") else {
            return Ok(vec![n.encode()]);
        };
        if n.attrs.remove(at).i != Some(1) {
            return Err(self.unsupported(
                "BatchNormalization with spatial = 0 (per-element statistics) has no \
                 opset-9 form"
                    .to_owned(),
            ));
        }
        self.rewritten += 1;
        Ok(vec![n.encode()])
    }

    fn slice(&mut self, mut n: Node<'_>) -> Result<Vec<Vec<u8>>, UpgradeError> {
        if n.inputs.len() != 1 {
            return Err(self.unsupported(format!(
                "an opset-{} Slice has one input, this one has {}",
                self.opset,
                n.inputs.len()
            )));
        }
        let take = |n: &mut Node<'_>, name: &[u8]| {
            n.attrs
                .iter()
                .position(|a| a.name == name)
                .map(|i| n.attrs.remove(i).ints)
        };
        let (Some(starts), Some(ends)) = (take(&mut n, b"starts"), take(&mut n, b"ends")) else {
            return Err(self.unsupported("a Slice without starts or ends".to_owned()));
        };
        let axes = take(&mut n, b"axes");
        let id = self.fresh();
        let mut nodes = Vec::with_capacity(4);
        let mut constant = |suffix: &str, values: &[i64], inputs: &mut Vec<Vec<u8>>| {
            let name = format!("pdfcer_opset_upgrade_{id}_{suffix}").into_bytes();
            nodes.push(constant_i64(&name, values));
            inputs.push(name);
        };
        let mut extra = Vec::new();
        constant("starts", &starts, &mut extra);
        constant("ends", &ends, &mut extra);
        if let Some(axes) = &axes {
            constant("axes", axes, &mut extra);
        }
        n.owned_inputs = extra;
        nodes.push(n.encode());
        self.rewritten += 1;
        Ok(nodes)
    }

    fn softmax(&mut self, mut n: Node<'_>) -> Result<Vec<Vec<u8>>, UpgradeError> {
        if self.opset >= TARGET_OPSET {
            return Ok(vec![n.encode()]);
        }
        let (&[x], &[y]) = (n.inputs.as_slice(), n.outputs.as_slice()) else {
            return Err(self.unsupported(format!(
                "a {} node needs one input and one output",
                String::from_utf8_lossy(n.op_type)
            )));
        };
        let axis = match n.attrs.iter().position(|a| a.name == b"axis") {
            Some(i) => n.attrs.remove(i).i.unwrap_or(1),
            None => 1,
        };
        let id = self.fresh();
        let name = |s: &str| format!("pdfcer_opset_upgrade_{id}_{s}").into_bytes();
        let (shape, flat, two_d) = (name("shape"), name("flat"), name("2d"));

        let shape_node = simple_node(b"Shape", &[x], &shape, &[]);
        let flatten = simple_node(b"Flatten", &[x], &flat, &[int_attr(b"axis", axis)]);
        n.owned_outputs.clear();
        n.inputs.clear();
        n.outputs.clear();
        n.owned_inputs.push(flat);
        n.owned_outputs.push(two_d.clone());
        n.new_attrs.push(int_attr(b"axis", 1));
        let op = n.encode();
        let reshape = simple_node(b"Reshape", &[&two_d, &shape], y, &[]);
        self.rewritten += 1;
        Ok(vec![shape_node, flatten, op, reshape])
    }

    fn fresh(&mut self) -> usize {
        self.next_id += 1;
        self.next_id
    }
}

/// A `NodeProto`, split into the fields a rewrite touches and the rest
/// (kept as raw encoded bytes).
struct Node<'a> {
    inputs: Vec<&'a [u8]>,
    /// Inputs appended after `inputs` (owned, for new constants).
    owned_inputs: Vec<Vec<u8>>,
    outputs: Vec<&'a [u8]>,
    /// Outputs appended after `outputs` (owned, for renamed outputs).
    owned_outputs: Vec<Vec<u8>>,
    op_type: &'a [u8],
    domain: &'a [u8],
    attrs: Vec<Attr<'a>>,
    /// Encoded `AttributeProto` bodies added by a rewrite.
    new_attrs: Vec<Vec<u8>>,
    rest: Vec<&'a [u8]>,
}

struct Attr<'a> {
    raw: &'a [u8],
    /// Replacement body when a subgraph inside was rewritten.
    owned: Option<Vec<u8>>,
    name: &'a [u8],
    i: Option<i64>,
    ints: Vec<i64>,
    has_graph: bool,
}

impl<'a> Node<'a> {
    fn parse(body: &'a [u8]) -> Result<Self, UpgradeError> {
        let mut n = Node {
            inputs: Vec::new(),
            owned_inputs: Vec::new(),
            outputs: Vec::new(),
            owned_outputs: Vec::new(),
            op_type: b"",
            domain: b"",
            attrs: Vec::new(),
            new_attrs: Vec::new(),
            rest: Vec::new(),
        };
        for field in Fields::new(body) {
            let field = field?;
            match (field.num, field.value) {
                (NODE_INPUT, Value::Len(s)) => n.inputs.push(s),
                (NODE_OUTPUT, Value::Len(s)) => n.outputs.push(s),
                (NODE_OP_TYPE, Value::Len(s)) => n.op_type = s,
                (NODE_DOMAIN, Value::Len(s)) => {
                    n.domain = s;
                    n.rest.push(field.raw);
                }
                (NODE_ATTRIBUTE, Value::Len(a)) => n.attrs.push(Attr::parse(a)?),
                _ => n.rest.push(field.raw),
            }
        }
        Ok(n)
    }

    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for s in &self.inputs {
            put_len(&mut out, NODE_INPUT, s);
        }
        for s in &self.owned_inputs {
            put_len(&mut out, NODE_INPUT, s);
        }
        for s in &self.outputs {
            put_len(&mut out, NODE_OUTPUT, s);
        }
        for s in &self.owned_outputs {
            put_len(&mut out, NODE_OUTPUT, s);
        }
        put_len(&mut out, NODE_OP_TYPE, self.op_type);
        for a in &self.attrs {
            put_len(
                &mut out,
                NODE_ATTRIBUTE,
                a.owned.as_deref().unwrap_or(a.raw),
            );
        }
        for a in &self.new_attrs {
            put_len(&mut out, NODE_ATTRIBUTE, a);
        }
        for raw in &self.rest {
            out.extend_from_slice(raw);
        }
        out
    }
}

impl<'a> Attr<'a> {
    fn parse(raw: &'a [u8]) -> Result<Self, UpgradeError> {
        let mut a = Attr {
            raw,
            owned: None,
            name: b"",
            i: None,
            ints: Vec::new(),
            has_graph: false,
        };
        for field in Fields::new(raw) {
            let field = field?;
            match (field.num, field.value) {
                (ATTR_NAME, Value::Len(s)) => a.name = s,
                (ATTR_I, Value::Varint(v)) => a.i = Some(as_i64(v)),
                (ATTR_INTS, Value::Varint(v)) => a.ints.push(as_i64(v)),
                (ATTR_INTS, Value::Len(packed)) => {
                    let mut pos = 0;
                    while pos < packed.len() {
                        a.ints.push(as_i64(read_varint(packed, &mut pos)?));
                    }
                }
                (ATTR_G | ATTR_GRAPHS, Value::Len(_)) => a.has_graph = true,
                _ => {}
            }
        }
        Ok(a)
    }
}

#[allow(clippy::cast_possible_wrap)] // int64 on the wire is two's complement
const fn as_i64(v: u64) -> i64 {
    v as i64
}

#[allow(clippy::cast_sign_loss)] // int64 on the wire is two's complement
const fn as_u64(v: i64) -> u64 {
    v as u64
}

fn int_attr(name: &[u8], value: i64) -> Vec<u8> {
    let mut a = Vec::new();
    put_len(&mut a, ATTR_NAME, name);
    put_key(&mut a, ATTR_I, WIRE_VARINT);
    put_varint(&mut a, as_u64(value));
    put_key(&mut a, ATTR_TYPE, WIRE_VARINT);
    put_varint(&mut a, ATTR_TYPE_INT);
    a
}

fn simple_node(op: &[u8], inputs: &[&[u8]], output: &[u8], attrs: &[Vec<u8>]) -> Vec<u8> {
    let mut n = Vec::new();
    for i in inputs {
        put_len(&mut n, NODE_INPUT, i);
    }
    put_len(&mut n, NODE_OUTPUT, output);
    put_len(&mut n, NODE_OP_TYPE, op);
    for a in attrs {
        put_len(&mut n, NODE_ATTRIBUTE, a);
    }
    n
}

/// A `Constant` node producing a 1-D int64 tensor named `output`.
fn constant_i64(output: &[u8], values: &[i64]) -> Vec<u8> {
    let mut t = Vec::new();
    put_key(&mut t, TENSOR_DIMS, WIRE_VARINT);
    put_varint(&mut t, values.len() as u64);
    put_key(&mut t, TENSOR_DATA_TYPE, WIRE_VARINT);
    put_varint(&mut t, TENSOR_INT64);
    let raw: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    put_len(&mut t, TENSOR_RAW_DATA, &raw);

    let mut a = Vec::new();
    put_len(&mut a, ATTR_NAME, b"value");
    put_len(&mut a, ATTR_T, &t);
    put_key(&mut a, ATTR_TYPE, WIRE_VARINT);
    put_varint(&mut a, ATTR_TYPE_TENSOR);

    let mut n = Vec::new();
    put_len(&mut n, NODE_OUTPUT, output);
    put_len(&mut n, NODE_OP_TYPE, b"Constant");
    put_len(&mut n, NODE_ATTRIBUTE, &a);
    n
}

// ---------------------------------------------------------------------------
// Protobuf wire format.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Value<'a> {
    Varint(u64),
    Len(&'a [u8]),
    Fixed,
}

struct Field<'a> {
    num: u32,
    value: Value<'a>,
    /// The whole encoded field, key included.
    raw: &'a [u8],
}

struct Fields<'a> {
    buf: &'a [u8],
    pos: usize,
    failed: bool,
}

impl<'a> Fields<'a> {
    const fn new(buf: &'a [u8]) -> Self {
        Fields {
            buf,
            pos: 0,
            failed: false,
        }
    }

    fn next_field(&mut self) -> Result<Field<'a>, UpgradeError> {
        let start = self.pos;
        let key = read_varint(self.buf, &mut self.pos)?;
        let num = u32::try_from(key >> 3)
            .ok()
            .filter(|&n| n != 0)
            .ok_or(UpgradeError::Malformed("field number out of range"))?;
        #[allow(clippy::cast_possible_truncation)] // masked to three bits
        let wire = (key & 7) as u8;
        let value = match wire {
            WIRE_VARINT => Value::Varint(read_varint(self.buf, &mut self.pos)?),
            WIRE_FIXED64 => {
                self.skip(8)?;
                Value::Fixed
            }
            WIRE_FIXED32 => {
                self.skip(4)?;
                Value::Fixed
            }
            WIRE_LEN => {
                let len = usize::try_from(read_varint(self.buf, &mut self.pos)?)
                    .map_err(|_| UpgradeError::Malformed("length out of range"))?;
                let begin = self.pos;
                self.skip(len)?;
                Value::Len(self.buf.get(begin..self.pos).unwrap_or_default())
            }
            _ => return Err(UpgradeError::Malformed("unsupported wire type")),
        };
        Ok(Field {
            num,
            value,
            raw: self.buf.get(start..self.pos).unwrap_or_default(),
        })
    }

    fn skip(&mut self, n: usize) -> Result<(), UpgradeError> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.buf.len())
            .ok_or(UpgradeError::Malformed("field runs past its end"))?;
        self.pos = end;
        Ok(())
    }
}

impl<'a> Iterator for Fields<'a> {
    type Item = Result<Field<'a>, UpgradeError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.pos >= self.buf.len() {
            return None;
        }
        let r = self.next_field();
        self.failed = r.is_err();
        Some(r)
    }
}

fn read_varint(buf: &[u8], pos: &mut usize) -> Result<u64, UpgradeError> {
    let mut v = 0u64;
    for shift in (0..70).step_by(7) {
        let b = *buf
            .get(*pos)
            .ok_or(UpgradeError::Malformed("truncated varint"))?;
        *pos += 1;
        v |= u64::from(b & 0x7f) << shift.min(63);
        if b & 0x80 == 0 {
            return Ok(v);
        }
    }
    Err(UpgradeError::Malformed("varint longer than ten bytes"))
}

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        #[allow(clippy::cast_possible_truncation)] // low seven bits
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    #[allow(clippy::cast_possible_truncation)] // < 0x80
    out.push(v as u8);
}

fn put_key(out: &mut Vec<u8>, num: u32, wire: u8) {
    put_varint(out, (u64::from(num) << 3) | u64::from(wire));
}

fn put_len(out: &mut Vec<u8>, num: u32, body: &[u8]) {
    put_key(out, num, WIRE_LEN);
    put_varint(out, body.len() as u64);
    out.extend_from_slice(body);
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    fn model(opset: i64, nodes: &[Vec<u8>]) -> Vec<u8> {
        let mut g = Vec::new();
        for n in nodes {
            put_len(&mut g, GRAPH_NODE, n);
        }
        let mut o = Vec::new();
        put_key(&mut o, OPSET_VERSION, WIRE_VARINT);
        put_varint(&mut o, as_u64(opset));
        let mut m = Vec::new();
        put_len(&mut m, MODEL_OPSET_IMPORT, &o);
        put_len(&mut m, MODEL_GRAPH, &g);
        m
    }

    fn op_types(bytes: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        for f in Fields::new(bytes) {
            if let (MODEL_GRAPH, Value::Len(g)) = (f.as_ref().unwrap().num, f.unwrap().value) {
                for n in Fields::new(g) {
                    if let Value::Len(n) = n.unwrap().value {
                        let node = Node::parse(n).unwrap();
                        out.push(String::from_utf8_lossy(node.op_type).into_owned());
                    }
                }
            }
        }
        out
    }

    #[test]
    fn an_opset_13_model_is_returned_untouched() {
        let softmax = simple_node(b"Softmax", &[b"x"], b"y", &[int_attr(b"axis", 1)]);
        let m = model(13, &[softmax]);
        let up = upgrade(m.clone()).unwrap();
        assert_eq!(up.bytes, m);
        assert_eq!((up.opset, up.rewritten), (Some(13), 0));
    }

    #[test]
    fn an_old_softmax_is_wrapped_in_flatten_and_reshape() {
        let softmax = simple_node(b"Softmax", &[b"x"], b"y", &[int_attr(b"axis", 1)]);
        let up = upgrade(model(7, &[softmax])).unwrap();
        assert_eq!(up.rewritten, 1);
        assert_eq!(
            op_types(&up.bytes),
            ["Shape", "Flatten", "Softmax", "Reshape"]
        );
    }

    #[test]
    fn spatial_zero_is_refused_and_spatial_one_is_dropped() {
        let bn = |s| {
            simple_node(
                b"BatchNormalization",
                &[b"x"],
                b"y",
                &[int_attr(b"spatial", s)],
            )
        };
        assert!(matches!(
            upgrade(model(7, &[bn(0)])),
            Err(UpgradeError::Unsupported { opset: 7, .. })
        ));
        let up = upgrade(model(7, &[bn(1)])).unwrap();
        assert_eq!(up.rewritten, 1);
        assert!(!up.bytes.windows(7).any(|w| w == b"spatial"));
    }

    #[test]
    fn truncated_input_is_malformed_not_a_panic() {
        let softmax = simple_node(b"Softmax", &[b"x"], b"y", &[]);
        let m = model(7, &[softmax]);
        for cut in 0..m.len() {
            let _ = upgrade(m[..cut].to_vec());
        }
        assert!(matches!(
            upgrade(vec![0x0a, 0xff]),
            Err(UpgradeError::Malformed(_))
        ));
    }
}
