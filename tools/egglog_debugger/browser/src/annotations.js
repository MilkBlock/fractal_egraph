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

// ---------------------------------------------------------------------------
// The editing half: which parts of the formula are renamable, and how a rename
// or a condition edit is written back as a `; @egg-viz-json` comment.
// ---------------------------------------------------------------------------

const BINARY_TEMPLATES = {
    Add: ["{left} + {right}", 50], Plus: ["{left} + {right}", 50],
    Sub: ["{left} - {right}", 50], Mul: ["{left} * {right}", 60],
    Times: ["{left} * {right}", 60], MMul: ["{left} * {right}", 60],
    Div: ["frac({left}, {right})", 60], Pow: ["{left}^({right})", 80],
    Kron: ["{left} ⊗ {right}", 60], Eq: ["{left} = {right}", 40]
};

export function defaultTemplate(name, arity) {
    if (arity === 2 && name in BINARY_TEMPLATES) {
        const [typst, precedence] = BINARY_TEMPLATES[name];
        return { fields: ["left", "right"], typst, precedence };
    }
    if (arity === 2 && name === "Diff") {
        return { fields: ["variable", "expression"], typst: "frac(d {expression}, d {variable})", precedence: 90 };
    }
    if (arity === 2 && name === "Integral") {
        return { fields: ["integrand", "variable"], typst: "integral {integrand} quad d {variable}", precedence: 90 };
    }
    if (arity === 1 && ["Num", "Const", "Lit", "Var", "NamedDim", "NamedMat"].includes(name)) {
        return { fields: ["value"], typst: "{value}", precedence: 100 };
    }
    if (arity === 1 && ["Sin", "Cos", "Ln", "Sqrt"].includes(name)) {
        return { fields: ["value"], typst: `${name.toLowerCase()}({value})`, precedence: 90 };
    }
    const fields = Array.from({ length: arity }, (_, index) => `arg${index}`);
    const label = `upright(${JSON.stringify(name)})`;
    const call = fields.length ? `(${fields.map(field => `{${field}}`).join(", ")})` : "";
    return { fields, typst: label + call, precedence: 90 };
}

// Actual declared symbols, never arities inferred from observed calls.
export function* declarations(forms) {
    for (const form of walk(forms)) {
        if (form.op === "datatype" && form.items.length >= 2) {
            for (const variant of form.items.slice(2)) {
                if (!variant.items) continue;
                const args = [];
                for (const arg of variant.items.slice(1)) {
                    if (arg.atom && arg.atom.startsWith(":")) break;
                    args.push(arg);
                }
                yield [form.items[1].atom, variant.op, args.length, form];
            }
        } else if (form.op === "datatype*") {
            for (const datatype of form.items.slice(1)) {
                if (!datatype.items || datatype.op === "sort") continue;
                for (const variant of datatype.items.slice(1)) {
                    if (!variant.items) continue;
                    const args = [];
                    for (const arg of variant.items.slice(1)) {
                        if (arg.atom && arg.atom.startsWith(":")) break;
                        args.push(arg);
                    }
                    yield [datatype.op, variant.op, args.length, form];
                }
            }
        } else if (["constructor", "function", "relation"].includes(form.op) && form.items.length >= 3) {
            yield [form.items[1].atom, form.items[1].atom, (form.items[2].items || []).length, form];
        }
    }
}

export function rustFields(symbol, form, arity) {
    const groups = [];
    if (form.op === "datatype") groups.push(form.items.slice(2));
    else if (form.op === "datatype*") {
        groups.push(form.items.slice(1).filter(item => item.items && item.op !== "sort").map(item => item.items.slice(1)));
    }
    for (const variants of groups) {
        for (const [index, variant] of variants.entries()) {
            if (variant.op !== symbol) continue;
            return variant.items.slice(1, 1 + arity)
                .map((arg, position) => `arg_${(arg.atom || "value").replace(/[^A-Za-z0-9_]/g, "_")}_${index}${position}`);
        }
    }
    return Array.from({ length: arity }, (_, index) => `arg${index}`);
}

// The extractor keys the rendered formula by `side/index/expr/position`; the
// same keys let the editor map a clicked word back to a rule binding.
export function ruleLabels(form, declared) {
    const bindings = {};
    const positions = {};
    if (form.op !== "rule" || form.items.length < 3) return { bindings, positions };
    // Call operators are not bindings.
    const parents = [...walk([form])];
    for (const atom of walk(form.items[1].items || [])) {
        const name = atom.atom;
        if (!name || atom.quoted) continue;
        if (!/^[A-Za-z_][\w-]*$/.test(name)) continue;
        if (declared.has(name) || ["true", "false", "nil", "_"].includes(name)) continue;
        if (parents.some(parent => parent.items && parent.items[0] === atom)) continue;
        bindings[name] = name;
    }
    for (const [side, group] of [["body", form.items[1]], ["head", form.items[2]]]) {
        (group.items || []).forEach((item, index) => {
            let expressions;
            if ((item.op === "=" && side === "body") || (item.op === "union" && side === "head")) {
                expressions = item.items.slice(1, 3);
            } else if (item.op === "head" && ["set", "let", "delete", "subsume", "panic"].includes(item.op)) {
                return;
            } else if (["set", "let", "delete", "subsume", "panic"].includes(item.op) && side === "head") {
                return;
            } else {
                expressions = [item];
            }
            expressions.forEach((expression, position) => {
                if (expression.op in declared || declared.has(expression.op)) {
                    const prefix = side === "body" ? "match" : "result";
                    positions[`${side}/${index}/expr/${position}`] = `${prefix}_${expression.op}_${index}_${position}`;
                }
            });
        });
    }
    return { bindings, positions };
}

function conditionParts(source, rule) {
    if (rule.op === "rewrite" || rule.op === "birewrite") {
        for (const [index, item] of rule.items.entries()) {
            if (item.atom === ":when" && index + 1 < rule.items.length) {
                return { facts: rule.items[index + 1].items || [], option: [item.start, rule.items[index + 1].end] };
            }
        }
        return { facts: [], option: null };
    }
    const declared = new Set([...declarations(parse(source))].map(([, name]) => name));
    const facts = [];
    for (const fact of rule.items[1].items || []) {
        // Declared table calls establish the match; primitive tests and scalar
        // equalities are explicit side conditions. Keep match facts untouched.
        if (![...walk([fact])].some(node => declared.has(node.op))) facts.push(fact);
    }
    return { facts, option: null };
}

function conditionTarget(source, rule) {
    const { facts } = conditionParts(source, rule);
    return {
        id: "condition:when", kind: "conditions", name: "规则附加条件", words: [],
        condition_source: facts.map(fact => source.slice(fact.start, fact.end)).join("\n"),
        form_kind: rule.op
    };
}

function literalLabels(template) {
    return [...String(template).matchAll(/(?:upright|op)\("((?:\\.|[^"\\])*)"\)/g)].map(match => JSON.parse(`"${match[1]}"`));
}

export function templatePlaceholders(template) {
    const names = [];
    for (let index = 0; index < template.length;) {
        const pair = template.slice(index, index + 2);
        if (pair === "{{" || pair === "}}") { index += 2; continue; }
        if (template[index] === "{") {
            const end = template.indexOf("}", index + 1);
            if (end < 0) throw new Error("模板占位符缺少右花括号 }");
            names.push(template.slice(index + 1, end));
            index = end + 1;
            continue;
        }
        if (template[index] === "}") throw new Error("模板中有未配对的右花括号 }");
        index += 1;
    }
    return names;
}

export function validateTemplate(template, fields) {
    if (typeof template !== "string" || !template.trim() || template.length > 4000) {
        throw new Error("Typst 模板必须是 1–4000 个字符的数学表达式");
    }
    for (const name of templatePlaceholders(template)) {
        if (!fields.includes(name)) throw new Error(`未声明的模板字段：{${name}}；可用字段：` + fields.join(", "));
    }
}

// Every editable word of the selected rule: one entry per constructor the rule
// uses, one per binding, and the side conditions as a single region.
export function catalog(source, line) {
    const forms = parse(source);
    const metadata = [...annotations(source)];
    const templates = {};
    for (const { value } of metadata) {
        if (value.kind !== "dsl_type") continue;
        for (const [name, meta] of Object.entries(value.variants || {})) templates[name] = meta;
    }
    const offset = ruleOffset(source, line);
    const rule = ruleAtOffset(forms, offset);
    const used = rule ? new Set([...walk([rule])].map(form => form.op)) : new Set();
    const result = [];
    const seen = new Set();
    for (const [owner, name, arity, form] of declarations(forms)) {
        if (seen.has(name) || !used.has(name)) continue;
        seen.add(name);
        const key = name.replace(/-/g, "_");
        const template = templates[key] ?? templates[name] ?? null;
        let fields = rustFields(name, form, arity);
        const effective = template ?? {
            fields,
            typst: `upright(${JSON.stringify(name)})` + (fields.length ? `(${fields.map(field => `{${field}}`).join(", ")})` : ""),
            precedence: 90
        };
        const words = [name, key];
        const operator = /^\{[^}]+\} ([+*\-]) \{[^}]+\}$/.exec(effective.typst || "");
        if (operator) words.push(operator[1]);
        words.push(...literalLabels(effective.typst || ""));
        result.push({
            id: `constructor:${name}`, kind: "constructor", name, owner, fields,
            template: effective.typst || "", template_fields: effective.fields || fields,
            precedence: effective.precedence ?? 90,
            display: (literalLabels(effective.typst || "") || [name])[0] ?? name,
            field_labels: (effective.fields || []).join() !== fields.join() ? effective.fields : defaultTemplate(name, arity).fields,
            words: [...new Set(words)]
        });
    }
    if (rule) {
        const declared = new Set([...declarations(forms)].map(([, name]) => name));
        let bindings;
        if (rule.op === "rule") bindings = Object.keys(ruleLabels(rule, declared).bindings);
        else if (rule.op === "rewrite" || rule.op === "birewrite") bindings = rewriteVariables(rule);
        else bindings = [];
        const previous = [...walk(forms)]
            .filter(form => ["rule", "rewrite", "birewrite"].includes(form.op) && form.end < rule.start)
            .reduce((best, form) => Math.max(best, form.end), -1);
        const attached = metadata
            .filter(({ start, value }) => previous < start && start < rule.start
                && ["original_rule", "combined_witness_bundle"].includes(value.kind))
            .map(({ value }) => value);
        const labels = attached.length ? (attached[attached.length - 1].labels || {}).bindings || {} : {};
        for (const name of bindings) {
            const display = labels[name] ?? name;
            result.push({
                id: `binding:${name}`, kind: "binding", name,
                words: [...new Set([name, display, String(display).replace(/[^A-Za-z0-9_]/g, "_")])],
                display
            });
        }
    }
    if (rule) result.push(conditionTarget(source, rule));
    return result;
}

// A display name may not collide with another constructor, binding or literal
// label; bindings are compared after the same normalisation the renderer uses.
function checkNameCollision(source, line, target, value) {
    if (target.words.includes(value)) return;
    const aliases = new Set();
    for (const other of catalog(source, line)) {
        if (other.id !== target.id) for (const word of other.words || []) aliases.add(word);
    }
    for (const [, name] of declarations(parse(source))) {
        if (target.kind !== "constructor" || name !== target.name) aliases.add(name), aliases.add(name.replace(/-/g, "_"));
    }
    for (const { value: meta } of annotations(source)) {
        if (meta.kind !== "dsl_type") continue;
        for (const [name, variant] of Object.entries(meta.variants || {})) {
            if (target.kind !== "constructor" || name !== target.name.replace(/-/g, "_")) {
                for (const label of literalLabels(variant.typst || "")) aliases.add(label);
            }
        }
    }
    const normalize = text => String(text).replace(/[^A-Za-z0-9_]/g, "_");
    const conflict = target.kind === "binding"
        ? aliases.has(value) || [...aliases].map(normalize).includes(normalize(value))
        : aliases.has(value);
    if (conflict) throw new Error(`显示名称重名：${value} 已被其他构造器或变量使用`);
}

export function updateDisplay(source, line, targetId, value, fields = null, template = null, precedence = null) {
    if (typeof value !== "string" || !value.trim() || value.length > 100 || /[\n\r]/.test(value)) {
        throw new Error("显示名称必须是 1–100 个字符的单行文本");
    }
    if (precedence !== null && (!Number.isInteger(precedence) || precedence < 0 || precedence > 65535)) {
        throw new Error("优先级必须是 0–65535 的整数");
    }
    if (fields !== null) {
        if (!Array.isArray(fields) || fields.some(field => typeof field !== "string" || !/^[A-Za-z_][A-Za-z0-9_]*$/.test(field))) {
            throw new Error("字段名称必须是合法的 Typst 模板标识符");
        }
    }
    const target = catalog(source, line).find(entry => entry.id === targetId);
    if (!target || target.kind === "conditions") throw new Error("所选文字与当前源码不匹配，请重新点击公式");
    const suppliedTemplate = template;
    if (suppliedTemplate === null && (value.includes("{") || value.includes("}"))) {
        throw new Error("显示名称不能包含花括号；完整公式请写在 Typst 模板里");
    }
    if (template === null || target.kind !== "constructor") checkNameCollision(source, line, target, value.trim());
    const forms = parse(source);
    const rows = [...annotations(source)];
    const offset = ruleOffset(source, line);
    const selected = [...walk(forms)].find(form =>
        ["rule", "rewrite", "birewrite"].includes(form.op) && form.start >= offset
        && source.slice(offset, form.start).trim() === "");
    let start = null, end = null, metadata = null, nextFields = null, effectivePrecedence = null, effectiveTemplate = template;
    if (target.kind === "constructor") {
        for (const row of rows) {
            if (row.value.kind === "dsl_type" && target.name.replace(/-/g, "_") in (row.value.variants || {})) {
                start = row.start; end = row.end; metadata = row.value;
            }
        }
        if (start === null) {
            const found = [...declarations(forms)].find(entry => entry[1] === target.name);
            start = end = found[3].start;
            metadata = { schema: "egg-viz/v1", kind: "dsl_type", id: found[0], variants: {} };
        }
        const actualFields = target.fields;
        const existing = metadata.variants[target.name.replace(/-/g, "_")]
            ?? { fields: actualFields, typst: target.template, precedence: 90 };
        const oldFields = existing.fields ?? actualFields;
        nextFields = fields !== null ? fields : oldFields;
        if (nextFields.length !== actualFields.length) throw new Error("字段数量不能改变");
        if (new Set(nextFields).size !== nextFields.length) throw new Error("字段名称不能重复");
        const mapping = new Map(oldFields.map((field, index) => [field, nextFields[index]]));
        effectiveTemplate = (suppliedTemplate !== null ? suppliedTemplate : existing.typst ?? "")
            .replace(/\{([A-Za-z_][A-Za-z0-9_]*)\}/g, (_, field) => `{${mapping.get(field) ?? field}}`);
        // Editing only fields preserves an existing infix/function template.
        if (suppliedTemplate === null && (fields === null || !(target.words || []).includes(value.trim()))) {
            effectiveTemplate = `upright(${JSON.stringify(value.trim())})`
                + (nextFields.length ? `(${nextFields.map(field => `{${field}}`).join(", ")})` : "");
        }
        validateTemplate(effectiveTemplate, nextFields);
        for (const label of literalLabels(effectiveTemplate)) checkNameCollision(source, line, target, label);
        effectivePrecedence = precedence !== null ? precedence : existing.precedence ?? 90;
        metadata.variants[target.name.replace(/-/g, "_")] =
            { ...existing, fields: nextFields, typst: effectiveTemplate, precedence: effectivePrecedence };
    } else {
        if (!selected) throw new Error("所选文字与当前源码不匹配，请重新点击公式");
        const previous = [...walk(forms)]
            .filter(form => ["rule", "rewrite", "birewrite"].includes(form.op) && form.end < selected.start)
            .reduce((best, form) => Math.max(best, form.end), -1);
        for (const row of rows) {
            if (previous < row.start && row.start < selected.start
                && ["original_rule", "combined_witness_bundle"].includes(row.value.kind)) {
                start = row.start; end = row.end; metadata = row.value;
            }
        }
        if (start === null) {
            start = end = selected.start;
            metadata = { schema: "egg-viz/v1", kind: "original_rule", id: `edited-rule-${line}`, labels: {} };
        }
        metadata.labels = metadata.labels || {};
        metadata.labels.bindings = metadata.labels.bindings || {};
        metadata.labels.bindings[target.name] = value.trim();
    }
    let text = PREFIX + JSON.stringify(metadata);
    if (start === end) text = (start === 0 || source[start - 1] === "\n" ? "" : "\n") + text + "\n";
    const updated = source.slice(0, start) + text + source.slice(end);
    const moved = selected.start + (start <= selected.start ? text.length - (end - start) : 0);
    return {
        source: updated,
        line: updated.slice(0, moved).split("\n").length,
        target: target.id,
        template: target.kind === "constructor" ? effectiveTemplate : null,
        template_fields: target.kind === "constructor" ? nextFields : null,
        precedence: effectivePrecedence
    };
}

const FORBIDDEN_CONDITIONS = ["rule", "rewrite", "birewrite", "datatype", "datatype*", "constructor",
    "relation", "function", "run", "run-schedule", "include", "set", "union", "delete",
    "subsume", "let", "panic", "push", "pop"];

export function updateConditions(source, line, conditions) {
    if (typeof conditions !== "string" || conditions.length > 10000) {
        throw new Error("条件必须是最多 10000 个字符的 .egg 文本");
    }
    const rule = ruleAtOffset(parse(source), ruleOffset(source, line));
    if (!rule) throw new Error("当前行没有可编辑的规则");
    const newFacts = parse(conditions);
    if (newFacts.some(fact => !fact.items || !fact.op || FORBIDDEN_CONDITIONS.includes(fact.op))) {
        throw new Error("请输入条件表达式，例如 (> a 0)，不要输入动作或程序命令");
    }
    // Reconstruct only submitted fact spans so trailing comments cannot consume
    // the closing parenthesis of :when or the surrounding rule.
    const body = newFacts.map(fact => conditions.slice(fact.start, fact.end)).join("\n");
    const { facts: oldFacts, option } = conditionParts(source, rule);
    const edits = [];
    if (rule.op === "rewrite" || rule.op === "birewrite") {
        if (option) edits.push([option[0], option[1], newFacts.length ? `:when (\n${body}\n)` : ""]);
        else if (newFacts.length) edits.push([rule.end - 1, rule.end - 1, ` :when (\n${body}\n)`]);
    } else {
        if (oldFacts.length) {
            oldFacts.forEach((fact, index) => edits.push([fact.start, fact.end, index === 0 ? body : ""]));
        } else if (newFacts.length) {
            edits.push([rule.items[1].end - 1, rule.items[1].end - 1, `\n${body}\n`]);
        }
    }
    let updated = source;
    for (const [start, end, text] of edits.sort((a, b) => b[0] - a[0])) {
        updated = updated.slice(0, start) + text + updated.slice(end);
    }
    return { source: updated, line: updated.slice(0, rule.start).split("\n").length, target: "condition:when" };
}

// ---------------------------------------------------------------------------
// Fractal lanes: the `.egg` a lane's preview is rendered from.
//
// A lane is one rule applied d times. Rather than assemble Typst for the
// intermediate states, generate the rule with its action applied 1..d times in
// the head and let the extractor render every state: it is the same pipeline
// that renders the rule itself, so constructor templates, `upright` names and
// precedence all come from the plugin.
// ---------------------------------------------------------------------------

const PATTERN_LITERALS = new Set(["true", "false", "nil", "_"]);

function isPatternVariable(form) {
    return form.items === null && !form.quoted && /^[A-Za-z_][\w-]*$/.test(form.atom || "")
        && !PATTERN_LITERALS.has(form.atom);
}

// The term the rule's action rewrites: the call on either side of the pattern's
// defining equality (or a bare call fact when there is no equality).
function patternTerm(body) {
    for (const item of walk([body])) {
        if (item === body || !item.items || !item.items.length) continue;
        if (item.op === "=" && item.items.length >= 3) {
            const [, lhs, rhs] = item.items;
            if (rhs.items && rhs.items.length) return rhs;
            if (lhs.items && lhs.items.length) return lhs;
        } else if (item.op !== "=" && item.op !== "<" && item.op !== ">") {
            return item;
        }
    }
    return null;
}

// How one application changes each pattern variable: `(A n limit)` rewritten to
// `(A (+ n 1) limit)` makes `n` stand for `(+ n 1)` and leaves `limit` alone.
function collectUpdate(pattern, action, update) {
    if (isPatternVariable(pattern)) {
        update.set(pattern.atom, action);
        return true;
    }
    if (pattern.items === null || !action || !action.items
        || pattern.op !== action.op || pattern.items.length !== action.items.length) return false;
    return pattern.items.slice(1).every((item, index) => collectUpdate(item, action.items[index + 1], update));
}

function renderExpr(form, environment, source) {
    if (form.items === null) {
        if (isPatternVariable(form) && environment.has(form.atom)) return environment.get(form.atom);
        return source.slice(form.start, form.end);
    }
    return `(${form.items.map(item => renderExpr(item, environment, source)).join(" ")})`;
}

function ruleNameOf(rule, source) {
    const items = rule.items || [];
    for (const [index, item] of items.entries()) {
        if (item.atom === ":name" && items[index + 1]) return items[index + 1].atom?.replace(/^"|"$/g, "") ?? null;
    }
    return null;
}

/// The source to preview a fractal lane with, plus the line of the generated
/// rule. `null` when the rule's shape cannot be unrolled (a ground rule, a
/// pattern that is not a call, a rewrite); the caller then previews the rule.
export function fractalRuleSource(source, line, depth, shown = 3) {
    if (!Number.isInteger(depth) || depth < 1) return null;
    const rule = ruleAtOffset(parse(source), ruleOffset(source, line));
    if (!rule || rule.op !== "rule" || !rule.items || rule.items.length < 3) return null;
    const body = rule.items[1];
    const head = rule.items[2];
    const action = head.items?.[0];
    if (!action || !action.items) return null;
    const pattern = patternTerm(body);
    if (!pattern) return null;
    const update = new Map();
    if (!collectUpdate(pattern, action, update) || !update.size) return null;

    // The first state is the pattern the lane triggers on; each later one is the
    // action with the update applied once more. Deep lanes collapse into one
    // ellipsis instead of unrolling 168 times.
    const spelled = Math.min(depth, shown);
    const states = [renderExpr(pattern, new Map(), source)];
    let environment = new Map();
    for (let step = 0; step < spelled; step++) {
        states.push(renderExpr(action, environment, source));
        environment = new Map([...update].map(([name, expr]) => [name, renderExpr(expr, environment, source)]));
    }
    const name = ruleNameOf(rule, source) ?? "rule";
    const ruleText = `; fractal lane ${name} ×${depth}\n(rule ${source.slice(body.start, body.end)}\n  (${states.join("\n   ")})\n  :name "fractal:${name}")`;
    const prefix = source.endsWith("\n") ? source : `${source}\n`;
    return {
        source: prefix + ruleText,
        line: prefix.split("\n").length,
        depth,
        states: spelled,
        truncated: spelled < depth
    };
}
