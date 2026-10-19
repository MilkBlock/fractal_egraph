import assert from 'node:assert/strict';
import {discoverySummary} from './cell-discovery.mjs';
assert.deepEqual(discoverySummary(),{available:false});
const d={contract:{outcomes:{indexed:{category:'recognized'},branching:{category:'outside-scope'}}},
  total_records:2,accounted_records:2,counts:{recognized:1,'outside-scope':1},by_reason:{indexed:1,branching:1},
  outcomes:[{record:0,reason:'indexed'},{record:1,reason:'branching'}]};
assert.equal(discoverySummary(d).complete,true);
for(const mutate of [
  x=>x.outcomes.pop(),
  x=>x.outcomes.push({...x.outcomes[1]}),
  x=>x.outcomes[1].record=0,
  x=>x.outcomes[1].record=9,
  x=>x.outcomes[0].reason='unknown',
  x=>x.counts.recognized=2,
  x=>x.by_reason.branching=0,
]) {const broken=structuredClone(d);mutate(broken);assert.equal(discoverySummary(broken).complete,false);}
console.log('Discovery accounting rejects omissions, duplicates, unknown outcomes and inconsistent totals');
