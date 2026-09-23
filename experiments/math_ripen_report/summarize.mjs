import fs from 'node:fs';
import {ripenCoverage,coverageSvg,coverageCsv} from '../../tools/egglog_debugger/ripen-coverage.mjs';
const [run,out]=process.argv.slice(2);fs.mkdirSync(out,{recursive:true});
const q=JSON.parse(fs.readFileSync(`${run}/analyze-11/saturated-rule-composition/queue.json`));
const cat=q.catalog.catalog,c= ripenCoverage(cat,q.cs);
fs.writeFileSync(`${out}/coverage.svg`,coverageSvg(c));fs.writeFileSync(`${out}/coverage.csv`,coverageCsv(c));
const counts={},reasons={},forms={};let aliases=0;
for(const j of q.jobs){const u=q.cs.units[j.candidate_id],kind=u.source_composition==null?'base':'composite';const key=`${kind}/${j.state}`;counts[key]=(counts[key]||0)+1;if(j.fully_equivalent_to!=null)aliases++;
 if(j.state==='Rejected'){let reason=j.reason||'';if(reason.startsWith('CS member '))reason='entry replay/check failed';reasons[reason]=(reasons[reason]||0)+1;}
}
for(const comp of q.cs.compositions){forms[comp.kind]=(forms[comp.kind]||0)+1;}
const rounds={};for(const j of q.jobs)if(j.state==='Running'){const n=j.ripen?.round??'unknown';rounds[n]=(rounds[n]||0)+1;}
const result={boundary:q.boundary,jobs:q.jobs.length,counts:q.counts,by_origin:counts,composition_forms:forms,rejection_reasons:reasons,fully_equivalent_alias_jobs:aliases,
 saturated_families:cat.saturated_rule_compositions,distinct_saturated_entry_texts:new Set(cat.triggers.map(t=>t.entry)).size,physical_saturated_jobs:q.jobs.filter(j=>j.state==='Saturated'&&j.fully_equivalent_to==null).length,
 coverage:{...c,known_layer_ids:undefined,rows:c.rows.map(({layer_ids,...r})=>r)},weak_relations:q.weak_dominate.length,reentry_observations:q.weak_dominate.filter(e=>e.reentry_observation).length,weak_unresolved:q.weak_unresolved.length,running_rounds:rounds,last_catalog_timings:cat.timings};
fs.writeFileSync(`${out}/summary.json`,JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify(result,null,2));
