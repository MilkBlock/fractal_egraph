// Measure editable glyph spans with Typst itself. Display remains the canonical
// plugin SVG; transparent hit rectangles are a separate interaction layer.
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { execFileSync } = require('node:child_process');

function editableRegions(source, targets, buildDocument) {
    if (!targets.length) return [];
    const byWord = new Map();
    for (const target of targets) for (const word of target.words) {
        if (!byWord.has(word)) byWord.set(word, []);
        if (!byWord.get(word).some(t => t.id === target.id)) byWord.get(word).push(target);
    }
    const condition=targets.find(target=>target.kind==='conditions');
    const marker=' quad upright("if") quad ';
    const boundary=condition?source.lastIndexOf(marker):-1;
    const prefix=boundary>=0?source.slice(0,boundary):source;
    let count = 0;
    let marked = prefix.replace(/\b(?:upright|op)\(("(?:\\.|[^"\\])*")\)|"(?:\\.|[^"\\])*"|[A-Za-z_][A-Za-z_0-9]*|[+*−-]/g, (token, quoted) => {
        const word = quoted ? JSON.parse(quoted) : token;
        const candidates = byWord.get(word) || [];
        if (candidates.length !== 1) return token;
        count++;
        return ` #eggedit(${JSON.stringify(candidates[0].id)}, ${JSON.stringify(word)})[$ ${token} $] `;
    });
    if(boundary>=0){
        const suffix=source.slice(boundary+' quad '.length);
        marked+=` quad #eggedit(${JSON.stringify(condition.id)}, "规则条件")[$ ${suffix} $] `;
        count++;
    }
    if (!count) return [];
    const prelude = `#let eggedit(id, word, body) = context {
  let size = measure(body)
  metadata((egg_edit: true, id: id, text: word, x: here().position().x.pt(), y: here().position().y.pt(), width: size.width.pt(), height: size.height.pt()))
  body
}\n`;
    const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'egg-typst-edit-'));
    try {
        const file = path.join(folder, 'hitboxes.typ');
        fs.writeFileSync(file, prelude + buildDocument(marked));
        const output = execFileSync(process.env.EGGPLANT_PATTERN_TYPST_PATH || 'typst',
            ['query', '--root', folder, file, 'metadata', '--field', 'value'],
            { encoding: 'utf8', timeout: 20000, maxBuffer: 4 * 1024 * 1024 });
        return JSON.parse(output).filter(r => r.egg_edit && targets.some(t => t.id === r.id))
            .map(r => ({ target_id: r.id, text: r.text, x: r.x, y: r.y-r.height, width: r.width, height: r.height }));
    } finally { fs.rmSync(folder, { recursive: true, force: true }); }
}
module.exports = { editableRegions };
