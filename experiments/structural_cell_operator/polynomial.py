"""Exact sparse arithmetic in Z[x_0,...]/(x_i**2 - x_i).

A monomial is the bit mask of its variables; zero is the constant monomial.
Integer coefficients are collected exactly, including signed cancellation.
These are functions on Boolean boundary assignments, not row counts or a
linear encoding of e-class IDs. Boolean-valued polynomials admit AND/product,
OR ``p + q - p*q``, and composition by Boolean-valued substitution.
"""

from collections.abc import Mapping


def _integer(value, what):
    if not isinstance(value, int):
        raise TypeError(f"{what} must be an exact integer")
    return int(value)


def _index(value):
    value = _integer(value, "variable index")
    if value < 0:
        raise ValueError("variable index must be nonnegative")
    return value


def _bits(mask):
    while mask:
        bit = mask & -mask
        yield bit.bit_length() - 1
        mask ^= bit


class Poly:
    """Immutable canonical square-free polynomial with exact integer coefficients."""

    __slots__ = ("_items",)

    def __init__(self, terms=None):
        if isinstance(terms, Poly):
            items = terms._items
        else:
            if terms is None:
                terms = {}
            elif isinstance(terms, int):
                terms = {0: terms}
            if not isinstance(terms, Mapping):
                raise TypeError("terms must map monomial masks to integer coefficients")
            normalized = {}
            for mask, coefficient in terms.items():
                mask = _integer(mask, "monomial mask")
                coefficient = _integer(coefficient, "coefficient")
                if mask < 0:
                    raise ValueError("monomial mask must be nonnegative")
                if coefficient:
                    normalized[mask] = coefficient
            items = tuple(sorted(normalized.items()))
        object.__setattr__(self, "_items", items)

    def __setattr__(self, name, value):
        raise AttributeError("Poly is immutable")

    @classmethod
    def zero(cls):
        return cls()

    @classmethod
    def one(cls):
        return cls(1)

    @classmethod
    def const(cls, value):
        return cls(_integer(value, "constant"))

    @classmethod
    def var(cls, index):
        return cls({1 << _index(index): 1})

    @property
    def terms(self):
        """Return a copy; callers cannot mutate the canonical representation."""
        return dict(self._items)

    @property
    def variables(self):
        return frozenset(i for mask, _ in self._items for i in _bits(mask))

    def is_zero(self):
        return not self._items

    def is_one(self):
        return self._items == ((0, 1),)

    def is_boolean(self):
        """Exact idempotence test; does not enumerate assignments."""
        return self * self == self

    def __bool__(self):
        """Whether the polynomial is nonzero, not its value in a particular world."""
        return bool(self._items)

    def __len__(self):
        return len(self._items)

    def __eq__(self, other):
        if isinstance(other, int):
            other = Poly(other)
        return isinstance(other, Poly) and self._items == other._items

    def __hash__(self):
        if not self._items:
            return hash(0)
        if len(self._items) == 1 and self._items[0][0] == 0:
            return hash(self._items[0][1])
        return hash(self._items)

    def __add__(self, other):
        other = Poly(other)
        result = self.terms
        for mask, coefficient in other._items:
            result[mask] = result.get(mask, 0) + coefficient
        return Poly(result)

    __radd__ = __add__

    def __neg__(self):
        return Poly({mask: -coefficient for mask, coefficient in self._items})

    def __sub__(self, other):
        return self + -Poly(other)

    def __rsub__(self, other):
        return Poly(other) + -self

    def __mul__(self, other):
        other = Poly(other)
        result = {}
        for left, a in self._items:
            for right, b in other._items:
                mask = left | right  # x_i**2 = x_i, preserving signed multiplicity.
                result[mask] = result.get(mask, 0) + a * b
        return Poly(result)

    __rmul__ = __mul__
    __and__ = __mul__
    __rand__ = __mul__

    def __pow__(self, exponent):
        exponent = _integer(exponent, "exponent")
        if exponent < 0:
            raise ValueError("negative polynomial powers are unsupported")
        result, base = Poly.one(), self
        while exponent:
            if exponent & 1:
                result = result * base
            base = base * base
            exponent >>= 1
        return result

    def __or__(self, other):
        """Inclusive OR for Boolean-valued operands, via inclusion-exclusion."""
        other = Poly(other)
        return self + other - self * other

    __ror__ = __or__

    def or_(self, other):
        return self | other

    def __invert__(self):
        return 1 - self

    def evaluate(self, assignment):
        """Evaluate a bit-mask world, or a complete variable -> {0,1} mapping."""
        if isinstance(assignment, int):
            if assignment < 0:
                raise ValueError("assignment bit mask must be nonnegative")
            mask = assignment
        else:
            if not isinstance(assignment, Mapping):
                assignment = dict(enumerate(assignment))
            missing = self.variables - assignment.keys()
            if missing:
                raise ValueError(f"missing Boolean assignments: {sorted(missing)}")
            mask = 0
            for index, value in assignment.items():
                index = _index(index)
                if not isinstance(value, int) or value not in (0, 1):
                    raise ValueError("assignment values must be Boolean")
                if value:
                    mask |= 1 << index
        return sum(coefficient for monomial, coefficient in self._items
                   if monomial & mask == monomial)

    eval = evaluate

    def restrict(self, variable, value):
        """Set one gate to 0 or 1, collecting newly identical monomials."""
        bit = 1 << _index(variable)
        if not isinstance(value, int) or value not in (0, 1):
            raise ValueError("restriction value must be Boolean")
        result = {}
        for mask, coefficient in self._items:
            if mask & bit and not value:
                continue
            mask &= ~bit
            result[mask] = result.get(mask, 0) + coefficient
        return Poly(result)

    def substitute(self, replacements, *, require_boolean=True):
        """Compose gate functions, simultaneously, then normalize.

        Keys are variable INDICES, not monomial masks. Unmentioned variables
        remain unchanged. Boolean substitutions preserve x_i**2=x_i; the default
        rejects non-idempotent replacements instead of claiming a ring map.
        ``require_boolean=False`` only substitutes into this canonical multilinear
        representative and must not be used as a Boolean operator composition.
        """
        if not isinstance(replacements, Mapping):
            replacements = dict(enumerate(replacements))
        replacements = {_index(i): Poly(p) for i, p in replacements.items()}
        if require_boolean and any(not p.is_boolean() for p in replacements.values()):
            raise ValueError("Boolean composition requires idempotent replacement polynomials")
        result = Poly.zero()
        for mask, coefficient in self._items:
            term = Poly.const(coefficient)
            for index in _bits(mask):
                term *= replacements.get(index, Poly.var(index))
            result += term
        return result

    def difference(self, variable):
        """Exact finite difference p[x_i=1] - p[x_i=0], not an infinitesimal."""
        return self.restrict(variable, 1) - self.restrict(variable, 0)

    delta = difference

    def coefficient_row(self, monomials):
        """Exact row in an explicit basis; never silently drop a nonzero column."""
        monomials = list(monomials)
        if any(not isinstance(mask, int) or mask < 0 for mask in monomials):
            raise ValueError("monomial basis requires nonnegative integer masks")
        if len(set(monomials)) != len(monomials):
            raise ValueError("duplicate monomial basis column")
        terms = self.terms
        if terms.keys() - set(monomials):
            raise ValueError("monomial basis omits a nonzero polynomial coefficient")
        return [terms.get(mask, 0) for mask in monomials]

    row = coefficient_row

    def to_json(self):
        return {"schema": "square-free-integer-polynomial/v1",
                "terms": [{"mask": mask, "coefficient": coefficient}
                          for mask, coefficient in self._items]}

    @classmethod
    def from_json(cls, value):
        if value.get("schema") != "square-free-integer-polynomial/v1":
            raise ValueError("unknown polynomial schema")
        terms = {}
        for term in value["terms"]:
            mask = _integer(term["mask"], "monomial mask")
            if mask in terms:
                raise ValueError("duplicate serialized monomial")
            terms[mask] = _integer(term["coefficient"], "coefficient")
        return cls(terms)

    def to_string(self, names=None):
        def name(index):
            if names is None:
                return f"x{index}"
            return str(names[index])

        parts = []
        for mask, coefficient in self._items:
            variables = "*".join(name(i) for i in _bits(mask))
            magnitude = abs(coefficient)
            term = (str(magnitude) if not variables else
                    variables if magnitude == 1 else f"{magnitude}*{variables}")
            if not parts:
                parts.append(("-" if coefficient < 0 else "") + term)
            else:
                parts.append((" - " if coefficient < 0 else " + ") + term)
        return "".join(parts) or "0"

    def __str__(self):
        return self.to_string()

    def __repr__(self):
        return f"Poly({self.terms!r})"


Polynomial = Poly
ZERO = Poly.zero()
ONE = Poly.one()


def or_delta(left, right, left_delta, right_delta):
    """Exact OR change, retaining the finite second-order cross term.

    OR(p+dp,q+dq)-OR(p,q) = dp+dq-p*dq-q*dp-dp*dq.
    The deltas can be signed and need not themselves be Boolean-valued.
    """
    p, q, dp, dq = map(Poly, (left, right, left_delta, right_delta))
    return dp + dq - p * dq - q * dp - dp * dq


def coefficient_matrix(polynomials, monomials=None):
    """Return (monomial_masks, exact_integer_rows), with the full column union."""
    polynomials = [Poly(p) for p in polynomials]
    if monomials is None:
        monomials = sorted({mask for p in polynomials for mask in p.terms})
    else:
        monomials = list(monomials)
    return monomials, [p.coefficient_row(monomials) for p in polynomials]
