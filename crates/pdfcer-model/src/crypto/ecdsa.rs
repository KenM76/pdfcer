//! ECDSA signature **verification** over NIST P-256 and P-384 (FIPS 186-4
//! §6.4.2, SEC 1 §4.1.4), on [`super::bignum`].
//!
//! Verification only, no secrets, not constant-time — the posture
//! `bignum.rs` argues. The two curves are the ones RFC 5480 §2.1.1.1
//! names and the ones every PDF signer in the wild uses; P-521 and the
//! Brainpool curves are reported as *unverifiable, by name*, not
//! approximated.
//!
//! # Arithmetic
//!
//! Jacobian projective coordinates `(X, Y, Z)` with `x = X/Z²`,
//! `y = Y/Z³`, the standard `a = −3` doubling and mixed addition formulas
//! (Guide to Elliptic Curve Cryptography, algorithms 3.21/3.22 adapted).
//! One field inversion per verification (to normalise the final point),
//! not one per point operation — that is the difference between a
//! millisecond and a second in a debug build.
//!
//! # What is checked (SEC 1 §4.1.4)
//!
//! 1. `r` and `s` in `[1, n−1]`;
//! 2. the public point is on the curve and not the identity (a key that is
//!    not a curve point is a malformed certificate, and the verification
//!    equation over a non-point proves nothing);
//! 3. `e` = the leftmost `bits(n)` bits of the digest (for P-256/SHA-256
//!    and P-384/SHA-384 that is the whole digest; for SHA-512 over P-256 it
//!    is truncated, per §4.1.4 step 2 / FIPS 186-4 §6.4);
//! 4. `u1 = e·s⁻¹`, `u2 = r·s⁻¹`, `R = u1·G + u2·Q`, accept iff `R ≠ ∞` and
//!    `R.x mod n == r`.

use super::bignum::Uint;

/// A named curve this verifier implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Curve {
    P256,
    P384,
}

/// The curve constants, all big-endian hex from FIPS 186-4 §D.1.2.
struct Params {
    p: Uint,
    a: Uint,
    b: Uint,
    gx: Uint,
    gy: Uint,
    n: Uint,
    /// Field element size in bytes.
    len: usize,
}

fn hex(s: &str) -> Uint {
    let bytes: Vec<u8> = (0..s.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect();
    Uint::from_be_bytes(&bytes)
}

impl Curve {
    /// The curve named by an RFC 5480 OID, if implemented.
    #[must_use]
    pub fn from_oid(oid: &str) -> Option<Self> {
        match oid {
            "1.2.840.10045.3.1.7" => Some(Self::P256),
            "1.3.132.0.34" => Some(Self::P384),
            _ => None,
        }
    }

    /// The curve's name, for a verdict.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::P256 => "P-256",
            Self::P384 => "P-384",
        }
    }

    fn params(self) -> Params {
        match self {
            Self::P256 => Params {
                p: hex("FFFFFFFF00000001000000000000000000000000FFFFFFFFFFFFFFFFFFFFFFFF"),
                a: hex("FFFFFFFF00000001000000000000000000000000FFFFFFFFFFFFFFFFFFFFFFFC"),
                b: hex("5AC635D8AA3A93E7B3EBBD55769886BC651D06B0CC53B0F63BCE3C3E27D2604B"),
                gx: hex("6B17D1F2E12C4247F8BCE6E563A440F277037D812DEB33A0F4A13945D898C296"),
                gy: hex("4FE342E2FE1A7F9B8EE7EB4A7C0F9E162BCE33576B315ECECBB6406837BF51F5"),
                n: hex("FFFFFFFF00000000FFFFFFFFFFFFFFFFBCE6FAADA7179E84F3B9CAC2FC632551"),
                len: 32,
            },
            Self::P384 => Params {
                p: hex(
                    "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEFFFFFFFF0000000000000000FFFFFFFF",
                ),
                a: hex(
                    "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEFFFFFFFF0000000000000000FFFFFFFC",
                ),
                b: hex(
                    "B3312FA7E23EE7E4988E056BE3F82D19181D9C6EFE8141120314088F5013875AC656398D8A2ED19D2A85C8EDD3EC2AEF",
                ),
                gx: hex(
                    "AA87CA22BE8B05378EB1C71EF320AD746E1D3B628BA79B9859F741E082542A385502F25DBF55296C3A545E3872760AB7",
                ),
                gy: hex(
                    "3617DE4A96262C6F5D9E98BF9292DC29F8F41DBD289A147CE9DA3113B5F0B8C00A60B1CE1D7E819D7A431D7C90EA0E5F",
                ),
                n: hex(
                    "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFC7634D81F4372DDF581A0DB248B0A77AECEC196ACCC52973",
                ),
                len: 48,
            },
        }
    }
}

/// A Jacobian point; `z == 0` is the point at infinity.
#[derive(Clone, Debug)]
struct Jac {
    x: Uint,
    y: Uint,
    z: Uint,
}

impl Jac {
    fn infinity() -> Self {
        Self {
            x: Uint::one(),
            y: Uint::one(),
            z: Uint::zero(),
        }
    }

    fn from_affine(x: &Uint, y: &Uint) -> Self {
        Self {
            x: x.clone(),
            y: y.clone(),
            z: Uint::one(),
        }
    }
}

struct Field<'a> {
    p: &'a Uint,
}

impl Field<'_> {
    fn mul(&self, a: &Uint, b: &Uint) -> Uint {
        a.mulmod(b, self.p)
    }
    fn sq(&self, a: &Uint) -> Uint {
        a.mulmod(a, self.p)
    }
    fn add(&self, a: &Uint, b: &Uint) -> Uint {
        a.addmod(b, self.p)
    }
    fn sub(&self, a: &Uint, b: &Uint) -> Uint {
        a.submod(b, self.p)
    }
    fn dbl(&self, a: &Uint) -> Uint {
        self.add(a, a)
    }

    /// Point doubling, `a = −3` (dbl-2001-b). Returns ∞ for ∞ or y = 0.
    fn double(&self, q: &Jac) -> Jac {
        if q.z.is_zero() || q.y.is_zero() {
            return Jac::infinity();
        }
        let zz = self.sq(&q.z);
        // M = 3(X − Z²)(X + Z²)   (uses a = −3)
        let m = {
            let t = self.mul(&self.sub(&q.x, &zz), &self.add(&q.x, &zz));
            self.add(&self.dbl(&t), &t)
        };
        let yy = self.sq(&q.y);
        // S = 4·X·Y²
        let s = {
            let t = self.mul(&q.x, &yy);
            self.dbl(&self.dbl(&t))
        };
        let x3 = self.sub(&self.sq(&m), &self.dbl(&s));
        // Y3 = M(S − X3) − 8·Y⁴
        let y4_8 = {
            let t = self.sq(&yy);
            self.dbl(&self.dbl(&self.dbl(&t)))
        };
        let y3 = self.sub(&self.mul(&m, &self.sub(&s, &x3)), &y4_8);
        let z3 = self.dbl(&self.mul(&q.y, &q.z));
        Jac {
            x: x3,
            y: y3,
            z: z3,
        }
    }

    /// General Jacobian addition (add-2007-bl, without the a-dependence).
    fn add_points(&self, p: &Jac, q: &Jac) -> Jac {
        if p.z.is_zero() {
            return q.clone();
        }
        if q.z.is_zero() {
            return p.clone();
        }
        let z1z1 = self.sq(&p.z);
        let z2z2 = self.sq(&q.z);
        let u1 = self.mul(&p.x, &z2z2);
        let u2 = self.mul(&q.x, &z1z1);
        let s1 = self.mul(&p.y, &self.mul(&q.z, &z2z2));
        let s2 = self.mul(&q.y, &self.mul(&p.z, &z1z1));
        if u1 == u2 {
            return if s1 == s2 {
                self.double(p)
            } else {
                Jac::infinity()
            };
        }
        let h = self.sub(&u2, &u1);
        let r = self.sub(&s2, &s1);
        let hh = self.sq(&h);
        let hhh = self.mul(&hh, &h);
        let v = self.mul(&u1, &hh);
        // X3 = r² − H³ − 2V
        let x3 = self.sub(&self.sub(&self.sq(&r), &hhh), &self.dbl(&v));
        // Y3 = r(V − X3) − S1·H³
        let y3 = self.sub(&self.mul(&r, &self.sub(&v, &x3)), &self.mul(&s1, &hhh));
        let z3 = self.mul(&self.mul(&p.z, &q.z), &h);
        Jac {
            x: x3,
            y: y3,
            z: z3,
        }
    }

    /// `k·P` by double-and-add, most significant bit first.
    fn scalar_mul(&self, k: &Uint, pt: &Jac) -> Jac {
        let mut acc = Jac::infinity();
        for i in (0..k.bits()).rev() {
            acc = self.double(&acc);
            if k.bit(i) {
                acc = self.add_points(&acc, pt);
            }
        }
        acc
    }

    /// Affine x of a Jacobian point, or `None` for ∞.
    fn affine_x(&self, q: &Jac) -> Option<Uint> {
        if q.z.is_zero() {
            return None;
        }
        let zinv = q.z.invmod_prime(self.p);
        Some(self.mul(&q.x, &self.sq(&zinv)))
    }
}

/// An ECDSA public key: an uncompressed SEC1 point on `curve`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcPublicKey {
    pub curve: Curve,
    pub x: Uint,
    pub y: Uint,
}

impl EcPublicKey {
    /// From a SEC1 encoded point (`04 || X || Y`). Compressed points are
    /// refused: no PDF signer emits them and decompression is more code
    /// on the untrusted path.
    #[must_use]
    pub fn from_sec1(curve: Curve, point: &[u8]) -> Option<Self> {
        let len = curve.params().len;
        let (&tag, rest) = point.split_first()?;
        if tag != 0x04 || rest.len() != 2 * len {
            return None;
        }
        let (x, y) = rest.split_at(len);
        let key = Self {
            curve,
            x: Uint::from_be_bytes(x),
            y: Uint::from_be_bytes(y),
        };
        key.is_on_curve().then_some(key)
    }

    /// `y² == x³ + ax + b (mod p)`.
    fn is_on_curve(&self) -> bool {
        let pr = self.curve.params();
        let f = Field { p: &pr.p };
        if self.x >= pr.p || self.y >= pr.p {
            return false;
        }
        let lhs = f.sq(&self.y);
        let rhs = f.add(
            &f.add(&f.mul(&f.sq(&self.x), &self.x), &f.mul(&pr.a, &self.x)),
            &pr.b,
        );
        lhs == rhs
    }

    /// Verify a signature `(r, s)` (big-endian magnitudes, as decoded from
    /// the DER `ECDSA-Sig-Value`) over `digest`.
    #[must_use]
    pub fn verify(&self, digest: &[u8], r: &[u8], s: &[u8]) -> bool {
        let pr = self.curve.params();
        let r = Uint::from_be_bytes(r);
        let s = Uint::from_be_bytes(s);
        if r.is_zero() || s.is_zero() || r >= pr.n || s >= pr.n {
            return false;
        }
        if !self.is_on_curve() {
            return false;
        }
        // e: the leftmost bits(n) bits of the digest.
        let n_bits = pr.n.bits();
        let mut e = Uint::from_be_bytes(digest);
        let d_bits = digest.len() * 8;
        if d_bits > n_bits {
            e = e.shr(d_bits - n_bits);
        }
        let w = s.invmod_prime(&pr.n);
        let u1 = e.mulmod(&w, &pr.n);
        let u2 = r.mulmod(&w, &pr.n);
        let f = Field { p: &pr.p };
        let g = Jac::from_affine(&pr.gx, &pr.gy);
        let q = Jac::from_affine(&self.x, &self.y);
        let point = f.add_points(&f.scalar_mul(&u1, &g), &f.scalar_mul(&u2, &q));
        match f.affine_x(&point) {
            Some(x) => x.rem(&pr.n) == r,
            None => false,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn the_generators_are_on_their_curves_and_have_order_n() {
        for curve in [Curve::P256, Curve::P384] {
            let pr = curve.params();
            let g = EcPublicKey {
                curve,
                x: pr.gx.clone(),
                y: pr.gy.clone(),
            };
            assert!(g.is_on_curve(), "{}", curve.name());
            let f = Field { p: &pr.p };
            let ng = f.scalar_mul(&pr.n, &Jac::from_affine(&pr.gx, &pr.gy));
            assert!(
                ng.z.is_zero(),
                "n·G must be the identity on {}",
                curve.name()
            );
            // (n−1)·G == −G, so its x is G's x.
            let n1 = pr.n.checked_sub(&Uint::one()).unwrap();
            let x = f
                .affine_x(&f.scalar_mul(&n1, &Jac::from_affine(&pr.gx, &pr.gy)))
                .unwrap();
            assert_eq!(x, pr.gx);
        }
    }

    /// RFC 6979 A.2.5, P-256 with SHA-256 over "sample" — a published
    /// deterministic test vector, so `r`/`s` are known constants.
    #[test]
    fn rfc_6979_p256_sha256_sample_verifies() {
        let key = EcPublicKey {
            curve: Curve::P256,
            x: hex("60FED4BA255A9D31C961EB74C6356D68C049B8923B61FA6CE669622E60F29FB6"),
            y: hex("7903FE1008B8BC99A41AE9E95628BC64F2F1B20C2D7E9F5177A3C294D4462299"),
        };
        use sha2::Digest as _;
        let digest = sha2::Sha256::digest(b"sample");
        let r = hex("EFD48B2AACB6A8FD1140DD9CD45E81D69D2C877B56AAF991C34D0EA84EAF3716");
        let s = hex("F7CB1C942D657C41D436C7A1B6E29F65F3E900DBB9AFF4064DC4AB2F843ACDA8");
        assert!(key.verify(
            &digest,
            &r.to_be_bytes(32).unwrap(),
            &s.to_be_bytes(32).unwrap()
        ));
        // Any bit of r wrong → false.
        let mut bad = r.to_be_bytes(32).unwrap();
        bad[31] ^= 1;
        assert!(!key.verify(&digest, &bad, &s.to_be_bytes(32).unwrap()));
        // Wrong message → false.
        let other = sha2::Sha256::digest(b"test");
        assert!(!key.verify(
            &other,
            &r.to_be_bytes(32).unwrap(),
            &s.to_be_bytes(32).unwrap()
        ));
    }

    /// RFC 6979 A.2.6, P-384 with SHA-384 over "sample".
    #[test]
    fn rfc_6979_p384_sha384_sample_verifies() {
        let key = EcPublicKey {
            curve: Curve::P384,
            x: hex(
                "EC3A4E415B4E19A4568618029F427FA5DA9A8BC4AE92E02E06AAE5286B300C64DEF8F0EA9055866064A254515480BC13",
            ),
            y: hex(
                "8015D9B72D7D57244EA8EF9AC0C621896708A59367F9DFB9F54CA84B3F1C9DB1288B231C3AE0D4FE7344FD2533264720",
            ),
        };
        use sha2::Digest as _;
        let digest = sha2::Sha384::digest(b"sample");
        let r = hex(
            "94EDBB92A5ECB8AAD4736E56C691916B3F88140666CE9FA73D64C4EA95AD133C81A648152E44ACF96E36DD1E80FABE46",
        );
        let s = hex(
            "99EF4AEB15F178CEA1FE40DB2603138F130E740A19624526203B6351D0A3A94FA329C145786E679E7B82C71A38628AC8",
        );
        assert!(key.verify(
            &digest,
            &r.to_be_bytes(48).unwrap(),
            &s.to_be_bytes(48).unwrap()
        ));
    }

    #[test]
    fn a_point_off_the_curve_is_refused() {
        let pr = Curve::P256.params();
        let mut bytes = vec![0x04];
        bytes.extend(pr.gx.to_be_bytes(32).unwrap());
        let mut y = pr.gy.to_be_bytes(32).unwrap();
        y[0] ^= 0x01;
        bytes.extend(y);
        assert!(EcPublicKey::from_sec1(Curve::P256, &bytes).is_none());
        assert!(
            Curve::from_oid("1.3.132.0.35").is_none(),
            "P-521 is named unsupported"
        );
    }
}
