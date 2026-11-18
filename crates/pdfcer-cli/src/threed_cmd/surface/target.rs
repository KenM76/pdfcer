//! `KIND:X,Y`: what a `--measure-distance` pick stands for, and where.

/// What a pick stands for, as the flag spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Vertex,
    Edge,
    Face,
}

/// One `--measure-distance` operand.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Target {
    pub(crate) kind: Kind,
    pub(crate) at: [f64; 2],
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self.kind {
            Kind::Vertex => "vertex",
            Kind::Edge => "edge",
            Kind::Face => "face",
        };
        write!(f, "{kind}:{},{}", self.at[0], self.at[1])
    }
}

/// Clap parser for `vertex:X,Y`, `edge:X,Y` or `face:X,Y`.
pub(crate) fn parse_target(s: &str) -> Result<Target, String> {
    let bad = || format!("`{s}` is not like edge:80,45.5 (vertex, edge or face, then X,Y)");
    let (kind, at) = s.split_once(':').ok_or_else(bad)?;
    let kind = match kind.trim() {
        "vertex" => Kind::Vertex,
        "edge" => Kind::Edge,
        "face" => Kind::Face,
        _ => return Err(bad()),
    };
    let at = crate::arg_types::parse_point2(at).map_err(|_| bad())?;
    Ok(Target { kind, at })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_target_is_a_kind_and_a_position() {
        let t = parse_target("edge:80,-4.5").unwrap();
        assert_eq!((t.kind, t.at), (Kind::Edge, [80.0, -4.5]));
        assert_eq!(t.to_string(), "edge:80,-4.5");
        assert!(parse_target("line:1,2").is_err());
        assert!(parse_target("face 1,2").is_err());
        assert!(parse_target("vertex:1").is_err());
    }
}
