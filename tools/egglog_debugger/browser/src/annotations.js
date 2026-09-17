// The `.egg` annotation protocol, without a Python process.
//
// The bridge asks egglog-demo's `preview_annotations.py` for the parts of a
// preview that are not the plugin's business: which `rewrite`/`birewrite` form a
// line belongs to, the rule structure the plugin's selector cannot parse for it,
// and the attached `egg-viz/v1` annotation. This module is the same protocol in
// JavaScript, and `test_annotations_parity.mjs` checks it against the Python
// reference over every example program.

export const PREFIX = "; @egg-viz-json ";
export const GENERATOR = "egglog-demo-preview/v1";

class Form {
    constructor(start, end, atom = null, items = null, quoted = false) {
        this.start = start;
        this.end = end;
        this.atom = atom;
        this.items = items;
        this.quoted = quoted;
    }

    get op() {
        return this.items && this.items[0] && this.items[0].atom ? this.items[0].atom : "";
    }
}

const TOKEN = /;[^\n]*|"(?:\\.|[^"\\])*"|[()]|[^\s();"]+/g;

export function parse(source) {
    const roots = [];
    const stack = [];
    for (const token of source.matchAll(TOKEN)) {
        const text = token[0];
        if (text.startsWith(";")) continue;
        const start = token.index;
        const parent = stack.length ? stack[stack.length - 1].items : roots;
        if (text === "(") {
            const node = new Form(start, start + 1, null, []);
            parent.push(node);
            stack.push(node);
        } else if (text === ")") {
            if (!stack.length) throw new Error("Unmatched closing parenthesis");
            stack.pop().end = start + 1;
        } else {
            parent.push(new Form(start, start + text.length, text, null, text.startsWith('"')));
        }
    }
    if (stack.length) throw new Error("Unclosed expression");
    return roots;
}

export function* walk(forms) {
    for (const form of forms) {
        yield form;
        if (form.items) yield* walk(form.items);
    }
}

export function* annotations(source) {
    for (const match of source.matchAll(/^[ \t]*; @egg-viz-json ([^\n]*)/gm)) {
        let value;
        try { value = JSON.parse(match[1]); } catch { continue; }
        if (value && typeof value === "object" && !Array.isArray(value)) {
            yield { start: match.index, end: match.index + match[0].length, value };
        }
    }
}

// Byte offset of the first character of a 1-based line, including the
// indentation the native runtime skips before the form itself.
function ruleOffset(source, line) {
    const lines = source.split("\n").slice(0, line - 1);
    let offset = lines.reduce((total, text) => total + text.length + 1, 0);
    const rest = source.slice(offset);
    offset += rest.length - rest.replace(/^\s+/, "").length;
    return offset;
}

// `parse` is not free on long programs, so the helpers take already parsed forms.
export function ruleAtOffset(forms, offset) {
    return [...walk(forms)].find(form =>
        (form.op === "rule" || form.op === "rewrite" || form.op === "birewrite")
        && form.start <= offset && offset < form.end) ?? null;
}

export function eggExpr(form) {
    if (form.items === null) {
        const text = form.atom || "";
        if (form.quoted) {
            let value;
            try { value = JSON.parse(text); } catch { value = text.replace(/^"|"$/g, ""); }
            return { type: "literal", value };
        }
        if (/^-?\d[\d_]*$/.test(text) || ["true", "false", "nil"].includes(text)) {
            return { type: "literal", value: text };
        }
        return { type: "var", name: text };
    }
    if (form.items.length && form.items[0].items !== null) {
        return { type: "list", items: form.items.map(item => eggExpr(item)) };
    }
    const op = form.items.length ? form.items[0].atom : "";
    return { type: "call", op, args: form.items.slice(1).map(item => eggExpr(item)) };
}

function* structureVariables(node) {
    if (Array.isArray(node)) { for (const item of node) yield* structureVariables(item); return; }
    if (node && typeof node === "object") {
        if (node.type === "var") yield node.name;
        for (const key of ["args", "items", "body", "head"]) {
            for (const child of node[key] || []) yield* structureVariables(child);
        }
        for (const key of ["lhs", "rhs", "expr"]) {
            if (key in node) yield* structureVariables(node[key]);
        }
    }
}

export function rewriteVariables(rule) {
    const nodes = [];
    rule.items.forEach((item, index) => {
        if (item.atom === ":when" && index + 1 < rule.items.length) nodes.push(eggExpr(item));
        else if (index === 1 || index === 2) nodes.push(eggExpr(item));
    });
    const names = nodes.flatMap(node => [...structureVariables(node)]).filter(name => name !== "__viz_root");
    return [...new Set(names)];
}

function attachedRuleAnnotation(forms, source, rule) {
    const previous = [...walk(forms)]
        .filter(form => (form.op === "rule" || form.op === "rewrite" || form.op === "birewrite") && form.end < rule.start)
        .reduce((best, form) => Math.max(best, form.end), -1);
    for (const { start, value } of annotations(source)) {
        if (previous < start && start < rule.start
            && ["original_rule", "combined_witness_bundle"].includes(value.kind)) return value;
    }
    return {};
}

// Rule structure for a rewrite (or a birewrite's forward direction). The
// installed plugin parses explicit `rule` forms only, so the renderer injects
// this structure at the rule's ordinal before extracting.
export function rewritePreviewEntry(source, line) {
    const forms = parse(source);
    const rule = ruleAtOffset(forms, ruleOffset(source, line));
    if (!rule || (rule.op !== "rewrite" && rule.op !== "birewrite") || rule.items.length < 3) return null;
    const root = { type: "var", name: "__viz_root" };
    const body = [{ type: "eq", lhs: root, rhs: eggExpr(rule.items[1]) }];
    rule.items.forEach((item, index) => {
        if (item.atom === ":when" && index + 1 < rule.items.length) {
            for (const fact of (rule.items[index + 1].items || [])) body.push({ type: "fact", expr: eggExpr(fact) });
        }
    });
    const head = [{ type: "union", lhs: root, rhs: eggExpr(rule.items[2]) }];
    const bindings = [...structureVariables({ body, head })].filter(name => name !== "__viz_root");
    return {
        structure: { body, head, bindings: [...new Set(bindings)], name: null },
        annotation: attachedRuleAnnotation(forms, source, rule)
    };
}

// Source to hand the plugin for this line. `birewrite` is transpiled to a raw
// `parse_and_run_program` string with no `add_rule` scope, so swapping the
// keyword for `rewrite` previews the forward direction the desugaring uses.
export function previewSource(source, line) {
    const forms = parse(source);
    const rule = ruleAtOffset(forms, ruleOffset(source, line));
    if (!rule || rule.op !== "birewrite" || !rule.items.length) return source;
    const atom = rule.items[0];
    if (atom.atom !== "birewrite" || atom.start === null) return source;
    return source.slice(0, atom.start) + "rewrite" + source.slice(atom.end);
}

// Everything `/api/preview` adds to a request before the plugin renders it.
export function previewRequest(source, line) {
    return {
        rule_entry: rewritePreviewEntry(source, line),
        preview_source: previewSource(source, line)
    };
}
