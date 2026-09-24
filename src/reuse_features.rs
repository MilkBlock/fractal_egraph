//! Immutable, conservative fragment filters. A positive is never a match certificate.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Probe {
    No,
    Maybe,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    /// Every requested atom must be provided by this immutable stored body.
    BodyOnly,
    /// Query may read through an input, a parent enclosure, or an unknown graft.
    ThroughBoundary,
    UnknownConditions,
    StaleConnections,
}

/// Columns are arguments followed by an optional result. Literal strings supplied
/// directly must use native Literal::to_string encoding; from_query_atom normalizes
/// parsed literals. Unparsed guards require Region::UnknownConditions.
/// None denotes a variable,
/// not an unknown guard. Repeated variables add equality requirements; different
/// variables do NOT imply inequality. Arity excludes the result column.
#[derive(Clone, Debug)]
pub struct TypedAtom {
    pub op: String,
    pub arity: usize,
    pub columns: Vec<(String, Option<String>)>,
    pub equal_columns: Vec<(usize, usize)>,
}
impl TypedAtom {
    pub fn from_query_atom(
        query: &super::planner::Query,
        atom: usize,
        arity: usize,
    ) -> Option<Self> {
        let (op, ids) = query.atoms.get(atom)?;
        let mut parser = egglog::ast::Parser::default();
        let columns = ids
            .iter()
            .map(|i| {
                let (sort, literal) = query.values.get(*i)?;
                let literal = match literal {
                    None => None,
                    Some(text) => match parser.get_expr_from_string(None, text).ok()? {
                        egglog::ast::Expr::Lit(_, literal) => Some(literal.to_string()),
                        _ => return None,
                    },
                };
                Some((sort.clone(), literal))
            })
            .collect::<Option<Vec<_>>>()?;
        let equality_columns = ids.len().min(Features::EQUALITY_COLUMNS);
        let equal_columns = (0..equality_columns)
            .flat_map(|i| {
                (i + 1..equality_columns).filter_map(move |j| (ids[i] == ids[j]).then_some((i, j)))
            })
            .collect();
        let result = Self {
            op: op.clone(),
            arity,
            columns,
            equal_columns,
        };
        result.valid().then_some(result)
    }
    fn valid(&self) -> bool {
        self.columns
            .len()
            .checked_sub(self.arity)
            .is_some_and(|n| n <= 1)
            && self.columns.iter().all(|(sort, _)| !sort.is_empty())
            && self
                .equal_columns
                .iter()
                .all(|(a, b)| *a < self.columns.len() && *b < self.columns.len())
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Features {
    bits: [u64; 4],
    typed: [u64; 8],
    positional: [u64; 8],
}
impl Features {
    /// Bound equality-position work per row. Queries beyond this prefix simply
    /// skip that feature; absence of unsupported features can never yield No.
    pub const EQUALITY_COLUMNS: usize = 4;
    pub const STORAGE_BYTES: usize = 160;

    pub fn bake(body: &RipSatEgraph) -> Self {
        Self::bake_state(body.graph())
    }
    pub(crate) fn bake_state(body: &SaturatedRuleComposition) -> Self {
        let mut out = Self {
            bits: [0; 4],
            typed: [0; 8],
            positional: [0; 8],
        };
        for row in &body.rows {
            let seed = Self::seed(&row.op, row.args.len());
            Self::insert(&mut out.bits, seed);
            let columns: Vec<_> = row.args.iter().copied().chain([row.result]).collect();
            for (i, v) in columns.iter().enumerate() {
                let vertex = &body.values[*v];
                Self::insert(&mut out.typed, Self::feature(seed, 1, i, 0, &vertex.sort));
                if let Some(literal) = &vertex.literal {
                    Self::insert(&mut out.positional, Self::feature(seed, 2, i, 0, literal));
                }
            }
            for i in 0..columns.len().min(Self::EQUALITY_COLUMNS) {
                for j in i + 1..columns.len().min(Self::EQUALITY_COLUMNS) {
                    if columns[i] == columns[j] {
                        Self::insert(&mut out.positional, Self::feature(seed, 3, i, j, ""));
                    }
                }
            }
        }
        out
    }
    /// Union filters for a library-level negative gate. A hit may span different
    /// bodies, so exact per-fragment matching is still mandatory.
    pub fn merge(&mut self, other: &Self) {
        for (a, b) in self.bits.iter_mut().zip(other.bits) {
            *a |= b;
        }
        for (a, b) in self.typed.iter_mut().zip(other.typed) {
            *a |= b;
        }
        for (a, b) in self.positional.iter_mut().zip(other.positional) {
            *a |= b;
        }
    }
    fn seed(op: &str, arity: usize) -> u64 {
        // Stable FNV; OR membership, never XOR (duplicates cannot cancel).
        let mut h = 14695981039346656037u64;
        for b in op.bytes().chain([0]).chain((arity as u64).to_le_bytes()) {
            h = (h ^ u64::from(b)).wrapping_mul(1099511628211);
        }
        h
    }
    fn feature(mut h: u64, tag: u8, a: usize, b: usize, text: &str) -> u64 {
        for byte in [tag]
            .into_iter()
            .chain((a as u64).to_le_bytes())
            .chain((b as u64).to_le_bytes())
            .chain(text.bytes())
        {
            h = (h ^ u64::from(byte)).wrapping_mul(1099511628211);
        }
        h
    }
    fn positions(h: u64, bits: usize) -> [usize; 3] {
        [
            (h as usize) % bits,
            (h.rotate_right(21) as usize) % bits,
            (h.rotate_right(43) as usize) % bits,
        ]
    }
    fn insert(bits: &mut [u64], h: u64) {
        for p in Self::positions(h, bits.len() * 64) {
            bits[p / 64] |= 1 << (p % 64);
        }
    }
    fn contains(bits: &[u64], h: u64) -> bool {
        Self::positions(h, bits.len() * 64)
            .iter()
            .all(|p| bits[p / 64] & (1 << (p % 64)) != 0)
    }
    pub fn probe<'a>(
        &self,
        required: impl IntoIterator<Item = (&'a str, usize)>,
        region: Region,
    ) -> Probe {
        if region != Region::BodyOnly {
            return Probe::Maybe;
        }
        if required
            .into_iter()
            .any(|(op, arity)| !Self::contains(&self.bits, Self::seed(op, arity)))
        {
            Probe::No
        } else {
            Probe::Maybe
        }
    }
    pub fn probe_typed(&self, required: &[TypedAtom], region: Region) -> Probe {
        // Validate the entire request before rejecting anything. Unknown or
        // malformed requests are not negative evidence about a body.
        if region != Region::BodyOnly || required.iter().any(|a| !a.valid()) {
            return Probe::Maybe;
        }
        for atom in required {
            let seed = Self::seed(&atom.op, atom.arity);
            if !Self::contains(&self.bits, seed) {
                return Probe::No;
            }
            for (i, (sort, literal)) in atom.columns.iter().enumerate() {
                if !Self::contains(&self.typed, Self::feature(seed, 1, i, 0, sort)) {
                    return Probe::No;
                }
                if let Some(lit) = literal {
                    if !Self::contains(&self.positional, Self::feature(seed, 2, i, 0, lit)) {
                        return Probe::No;
                    }
                }
            }
            for &(a, b) in &atom.equal_columns {
                let (a, b) = (a.min(b), a.max(b));
                if a != b
                    && b < Self::EQUALITY_COLUMNS
                    && !Self::contains(&self.positional, Self::feature(seed, 3, a, b, ""))
                {
                    return Probe::No;
                }
            }
        }
        Probe::Maybe
    }
}
