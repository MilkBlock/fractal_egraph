import fs from 'node:fs';
import path from 'node:path';
import {ripenCoverage,coverageCsv,coverageSvg} from './ripen-coverage.mjs';
const [run,out]=process.argv.slice(2);if(!run||!out)throw Error('Usage: node export-ripen-coverage.mjs RUN_DIR NEW_OUTPUT_DIR');
const q=JSON.parse(fs.readFileSync(path.join(run,'saturated-rule-composition/queue.json'),'utf8'));
const d=ripenCoverage(q.catalog?.catalog,q.cs);if(d.status!=='ok')throw Error('Run has no catalog/CS context');
fs.mkdirSync(out,{recursive:false});
fs.writeFileSync(path.join(out,'top100.json'),JSON.stringify(d,null,2));fs.writeFileSync(path.join(out,'top100.csv'),coverageCsv(d));fs.writeFileSync(path.join(out,'top100.svg'),coverageSvg(d));
console.log(JSON.stringify({states:d.total_states,known_layers:d.known_layers,verified_layers:d.verified_layers,coverage:d.rows.at(-1)?.coverage}));
