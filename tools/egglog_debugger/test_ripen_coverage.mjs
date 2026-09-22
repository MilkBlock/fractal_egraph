import assert from 'node:assert/strict';
import {ripenCoverage,coverageCsv,coverageSvg} from './ripen-coverage.mjs';
const cs={units:[{coarse_layer:0,coarse:[1],smooth:[2],source_composition:null},{coarse_layer:0,coarse:[1],smooth:[2,3],source_composition:null},{coarse_layer:1,coarse:[4],smooth:[5],source_composition:null},{coarse_layer:2,coarse:[6],smooth:[],source_composition:null}]};
const trigger=(state,members,entry='same')=>({saturated_rule_composition:state,binding_origin:{members},entry});
const c={state_groups:[{saturated_rule_composition:0},{saturated_rule_composition:1}],triggers:[trigger(0,[1,2]),trigger(0,[1,2]),trigger(0,[1,2,3],'different'),trigger(0,[4,5]),trigger(1,[1,2]),trigger(1,[1,2,4,5])]};
const d=ripenCoverage(c,cs);assert.equal(d.known_layers,3);assert.equal(d.rows[0].count,2);assert.equal(d.rows[0].distinct_entries,2);assert.equal(d.rows[1].count,1);assert.equal(d.rows[1].cumulative,2);assert.equal(d.unattributed_triggers,1);assert.equal(d.not_verified_layers,1);
assert.equal(ripenCoverage(c,null).status,'missing_cs_context');assert(coverageCsv(d).includes('0 1'));assert(coverageSvg(d).includes('data-state="0"'));
const many={units:Array.from({length:105},(_,i)=>({coarse_layer:i,coarse:[i],smooth:[],source_composition:null}))};const large={triggers:many.units.map((_,i)=>trigger(i,[i]))};const top=ripenCoverage(large,many);assert.equal(top.rows.length,100);assert.equal(top.rows.at(-1).cumulative,100);assert.equal(top.verified_layers,105);assert.equal(top.rows.at(-1).coverage,100/105);
console.log('Coverage ranking, deduplication, unknown denominator and composite non-attribution passed');
