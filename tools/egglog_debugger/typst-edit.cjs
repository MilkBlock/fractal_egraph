// Measure editable glyph spans with Typst itself. Display remains the canonical
// plugin SVG; transparent hit rectangles are a separate interaction layer.
//
// Only the measuring step is host specific (`typst query` under Node, the wasm
// compiler in the browser), so the marking step is shared with the browser
// bundle instead of being reimplemented there.

// Rewrite the Typst formula so every editable word is wrapped in `#eggedit`,
// which the prelude turns into metadata carrying its box. Returns null when no
// word in the source is editable.
function planEditableRegions(source, targets, buildDocument) {
    if (!targets.length) return null;
    const byWord = new Map();
    for (const target of targets) for (const word of target.words) {
        if (!byWord.has(word)) byWord.set(word, []);
        if (!byWord.get(word).some(t => t.id === target.id)) byWord.get(word).push(target);
    }
    const condition=targets.find(target=>target.kind==='conditions');
    const marker=' quad upright("if") quad ';
    const boundary=condition?source.lastIndexOf(marker):-1;
    const prefix=boundary>=0?source.slice(0,boundary):source;
    // A rendered variable can carry a field accessor (`num_node2.arg_i64_00`);
    // the editable target is the node before the dot, so the editor renames the
    // variable instead of writing the accessor as its label.
    const resolve = word => {
        if (byWord.has(word)) return { id: word, target: byWord.get(word) };
        const head = word.split('.')[0];
        return head !== word && byWord.has(head) ? { id: head, target: byWord.get(head) } : null;
    };
    let count = 0;
    let marked = prefix.replace(/\b(?:upright|op)\(("(?:\\.|[^"\\])*")\)|"(?:\\.|[^"\\])*"|[A-Za-z_][A-Za-z_0-9]*|[+*−-]/g, (token, quoted) => {
        const word = quoted ? JSON.parse(quoted) : token;
        const hit = resolve(word);
        if (!hit || hit.target.length !== 1) return token;
        count++;
        return ` #eggedit(${JSON.stringify(hit.target[0].id)}, ${JSON.stringify(hit.id)})[$ ${token} $] `;
    });
    if(boundary>=0){
        const suffix=source.slice(boundary+' quad '.length);
        marked+=` quad #eggedit(${JSON.stringify(condition.id)}, "规则条件")[$ ${suffix} $] `;
        count++;
    }
    if (!count) return null;
    const prelude = `#let eggedit(id, word, body) = context {
  let size = measure(body)
  metadata((egg_edit: true, id: id, text: word, x: here().position().x.pt(), y: here().position().y.pt(), width: size.width.pt(), height: size.height.pt()))
  body
}\n`;
    return { document: prelude + buildDocument(marked) };
}

// `typst query --field value` output, mapped to the hit rectangles the editor
// overlays. Shared by both hosts.
function collectEditableRegions(rows, targets) {
    return rows.filter(r => r.egg_edit && targets.some(t => t.id === r.id))
        .map(r => ({ target_id: r.id, text: r.text, x: r.x, y: r.y-r.height, width: r.width, height: r.height }));
}

function editableRegions(source, targets, buildDocument) {
    const plan = planEditableRegions(source, targets, buildDocument);
    if (!plan) return [];
    const fs = require('node:fs');
    const os = require('node:os');
    const path = require('node:path');
    const { execFileSync } = require('node:child_process');
    const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'egg-typst-edit-'));
    try {
        const file = path.join(folder, 'hitboxes.typ');
        fs.writeFileSync(file, plan.document);
        const output = execFileSync(process.env.EGGPLANT_PATTERN_TYPST_PATH || 'typst',
            ['query', '--root', folder, file, 'metadata', '--field', 'value'],
            { encoding: 'utf8', timeout: 20000, maxBuffer: 4 * 1024 * 1024 });
        return collectEditableRegions(JSON.parse(output), targets);
    } finally { fs.rmSync(folder, { recursive: true, force: true }); }
}

module.exports = { planEditableRegions, collectEditableRegions, editableRegions };
