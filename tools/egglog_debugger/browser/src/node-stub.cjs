// Throwing stand-ins for the Node builtins the shared preview core only
// references inside subprocess code paths that the browser never takes. They
// exist so the bundle has no unresolved `require("node:...")` calls.
const unavailable = name => () => { throw new Error(`浏览器构建不提供 ${name}`); };

module.exports = {
    existsSync: unavailable('node:fs.existsSync'),
    readFileSync: unavailable('node:fs.readFileSync'),
    writeFileSync: unavailable('node:fs.writeFileSync'),
    mkdtempSync: unavailable('node:fs.mkdtempSync'),
    rmSync: unavailable('node:fs.rmSync'),
    join: unavailable('node:path.join'),
    resolve: unavailable('node:path.resolve'),
    dirname: unavailable('node:path.dirname'),
    spawn: unavailable('node:child_process.spawn'),
    execFileSync: unavailable('node:child_process.execFileSync'),
    tmpdir: unavailable('node:os.tmpdir'),
    pathToFileURL: unavailable('node:url.pathToFileURL'),
    createHash: unavailable('node:crypto.createHash'),
    createRequire: unavailable('node:module.createRequire'),
    default: {}
};
