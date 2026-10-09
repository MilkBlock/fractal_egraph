"""Factor native surface AST conjuncts, preserving the complete named binding.

This is a source compiler, not a matcher or a runtime optimization claim.
Fresh emitted relations describe one snapshot: do not reuse them after updates.
No parser, name-specific CICM knowledge, cost budget, or search threshold is used.
"""
from collections import defaultdict
from dataclasses import dataclass
import json
import re

RESERVED = "__lhs_"
COMPARISONS = {"<", "<=", ">", ">=", "!="}


def expression_text(expr):
    """Render the native expression AST, independently of exported body_text."""
    kind = expr["kind"]
    if kind == "var":
        return expr["name"]
    if kind == "call":
        return "(" + " ".join([expr["head"], *map(expression_text, expr["args"])]) + ")"
    if kind == "literal":
        return expr["text"]
    raise ValueError(f"unsupported expression kind: {kind}")


def fact_text(fact):
    if fact["kind"] == "eq":
        return "(= " + " ".join(map(expression_text, fact["args"])) + ")"
    if fact["kind"] == "fact":
        return expression_text(fact["expr"])
    raise ValueError(f"unsupported fact kind: {fact['kind']}")


def body_text(body):
    return "(" + "\n ".join(map(fact_text, body)) + ")"


def expressions(fact):
    return fact["args"] if fact["kind"] == "eq" else [fact["expr"]]


def walk(expr):
    yield expr
    for child in expr.get("args", []):
        yield from walk(child)


def variables(fact):
    return {node["name"] for expr in expressions(fact) for node in walk(expr)
            if node["kind"] == "var"}


def infer_types(body, signatures):
    """Unify declaration/literal constraints; comparisons return egglog Unit."""
    parent, sorts, names, comparisons = [], [], {}, []

    def fresh(name):
        if name.startswith((RESERVED, "@", "$")):
            raise ValueError(f"reserved source variable: {name}")
        if name not in names:
            names[name] = len(parent)
            parent.append(len(parent))
            sorts.append(None)
        return names[name]

    def find(index):
        while parent[index] != index:
            index = parent[index]
        return index

    def unify(left, right):
        ls = left if isinstance(left, str) else sorts[find(left)]
        rs = right if isinstance(right, str) else sorts[find(right)]
        if ls is not None and rs is not None and ls != rs:
            raise ValueError(f"conflicting sorts: {ls} versus {rs}")
        if not isinstance(left, str):
            li = find(left)
            sorts[li] = ls or rs
            if not isinstance(right, str):
                parent[find(right)] = li
        elif not isinstance(right, str):
            sorts[find(right)] = ls or rs

    def infer(expr):
        kind = expr["kind"]
        if kind == "var":
            return fresh(expr["name"])
        if kind == "literal":
            expected = {"int": "i64", "float": "f64", "string": "String",
                        "bool": "bool", "unit": "Unit"}.get(expr["literal_kind"])
            if expected is None or expected != expr["sort"]:
                raise ValueError("unknown or inconsistent literal sort")
            return expected
        if kind != "call":
            raise ValueError(f"unsupported expression kind: {kind}")
        head, args = expr["head"], expr["args"]
        if head.startswith((RESERVED, "@", "$")):
            raise ValueError(f"reserved source head: {head}")
        if head in COMPARISONS:
            if head in signatures or len(args) != 2:
                raise ValueError(f"ambiguous comparison or invalid arity: {head}")
            left, right = map(infer, args)
            unify(left, right)
            comparisons.append((head, left))
            return "Unit"
        signature = signatures.get(head)
        if not isinstance(signature, dict) or signature.get("kind") not in {
                "constructor", "relation", "function"}:
            raise ValueError(f"unknown or ambiguous signature: {head}")
        if len(args) != len(signature["input"]):
            raise ValueError(f"wrong argument count: {head}")
        for arg, expected in zip(args, signature["input"]):
            unify(infer(arg), expected)
        return signature["output"]

    for fact in body:
        if fact["kind"] == "eq" and len(fact["args"]) == 2:
            unify(*map(infer, fact["args"]))
        elif fact["kind"] == "fact":
            # A surface fact may also test that a constructor/function call
            # exists; its implicit result need not have the Unit sort.
            infer(fact["expr"])
        else:
            raise ValueError("unsupported or malformed fact")
    result = {name: sorts[find(index)] for name, index in names.items()}
    if any(sort is None for sort in result.values()):
        raise ValueError("unresolved variable sort")
    for head, operand in comparisons:
        sort = operand if isinstance(operand, str) else sorts[find(operand)]
        if sort is None or (head != "!=" and sort not in {"i64", "f64"}):
            raise ValueError(f"unsupported or ambiguous comparison sort: {head} {sort}")
    return result


@dataclass
class Compilation:
    templates: list
    instances: list
    body: list
    metadata: dict
    occupied_names: set

    def emit(self, prefix):
        """Caller supplies a fresh prefix for every observed database snapshot."""
        if not re.fullmatch(r"[A-Za-z][A-Za-z0-9_-]*", prefix):
            raise ValueError("prefix must be a simple fresh egglog identifier")
        ruleset = prefix + "_produce"
        relations = [f"{prefix}_cell_{i}" for i in range(len(self.templates))]
        if self.occupied_names.intersection([ruleset, *relations]):
            raise ValueError("generated name collides with an existing declaration")
        declarations, producers = [f"(ruleset {ruleset})"], []
        for relation, template in zip(relations, self.templates):
            slots = template["slots"]
            declarations.append(f"(relation {relation} ({' '.join(s['sort'] for s in slots)}))")
            call = "(" + " ".join([relation, *(s["name"] for s in slots)]) + ")"
            producers.append(f"(rule {body_text(template['body'])} ({call}) :ruleset {ruleset})")
        replacements, consumed = {}, set()
        for instance in self.instances:
            indices = instance["source_indices"]
            replacements[min(indices)] = {"kind": "fact", "expr": {
                "kind": "call", "head": relations[instance["template"]],
                "args": instance["arguments"]}}
            consumed.update(indices)
        rewritten = [replacements[i] if i in replacements else fact
                     for i, fact in enumerate(self.body) if i in replacements or i not in consumed]
        return dict(declarations="\n".join(declarations), producer_rules="\n".join(producers),
                    ruleset=ruleset, rewritten_body=body_text(rewritten))


def compile_rule(rule, signatures):
    """Discover repeated directly bound constructor roots, then own conjuncts.

    Every complete source conjunct mentioning exactly one discovered root is
    assigned to that root. Zero-root and multi-root conjuncts stay residual.
    All named variables are exported; distinct boolean literal occurrences in
    declared table/constructor argument positions become explicit call slots.
    """
    body = rule["body"]
    types = infer_types(body, signatures)
    families = defaultdict(list)
    for fact in body:
        if fact["kind"] != "eq":
            continue
        for var, call in (fact["args"], fact["args"][::-1]):
            if var["kind"] == "var" and call["kind"] == "call" and signatures.get(
                    call["head"], {}).get("kind") == "constructor":
                if var["name"] not in families[call["head"]]:
                    families[call["head"]].append(var["name"])
    roots = {root for values in families.values() if len(values) > 1 for root in values}
    owned, residual = defaultdict(list), []
    for index, fact in enumerate(body):
        owners = variables(fact) & roots
        if len(owners) == 1:
            owned[next(iter(owners))].append(index)
        else:
            residual.append(index)
    templates, instances, cache = [], [], {}
    for root, indices in sorted(owned.items(), key=lambda item: item[1][0]):
        slots, arguments, renaming = [], [], {}

        def slot(argument, sort):
            name = f"{RESERVED}v{len(slots)}"
            slots.append(dict(name=name, sort=sort))
            arguments.append(argument)
            return {"kind": "var", "name": name}

        def canonical(expr, binding_position=False):
            kind = expr["kind"]
            if kind == "var":
                name = expr["name"]
                if name not in renaming:
                    renaming[name] = slot(expr, types[name])
                return renaming[name]
            if kind == "literal":
                return slot(expr, "bool") if binding_position and expr["sort"] == "bool" else dict(expr)
            return {"kind": "call", "head": expr["head"], "args": [
                canonical(arg, expr["head"] in signatures) for arg in expr["args"]]}

        normalized = []
        for index in indices:
            fact = body[index]
            normalized.append({"kind": "eq", "args": list(map(canonical, fact["args"]))}
                              if fact["kind"] == "eq" else
                              {"kind": "fact", "expr": canonical(fact["expr"])})
        template = dict(body=normalized, slots=slots)
        key = json.dumps(template, sort_keys=True, separators=(",", ":"))
        if key not in cache:
            cache[key] = len(templates)
            templates.append(template)
        instances.append(dict(root=root, source_indices=indices,
                              template=cache[key], arguments=arguments))
    metadata = dict(named_variables=list(types), variable_sorts=types,
                    root_families={k: v for k, v in families.items() if len(v) > 1},
                    residual_indices=residual, source_conjuncts=len(body),
                    owned_conjuncts=sum(len(i["source_indices"]) for i in instances),
                    templates=len(templates), instances=len(instances),
                    scope="source conjunct partition; all named bindings; fresh snapshot relations")
    occupied = set(signatures) | set(types) | {rule.get("ruleset", "")}
    return Compilation(templates, instances, body, metadata, occupied)
