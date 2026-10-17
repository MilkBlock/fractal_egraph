import assert from 'node:assert/strict';
import {model,pathTypst,bodyDot,overviewDot,familyDot} from './cell-egraph-panel.mjs';
const m=model({nodes:[
 {unit:0,eclass:0,contract:0,parents:[]},
 {unit:1,eclass:1,contract:1,parents:[0]},
 {unit:2,eclass:2,contract:2,parents:[]},
 {unit:3,eclass:3,contract:3,parents:[2]},
 {unit:4,eclass:3,contract:4,parents:[1]},
 {unit:5,eclass:0,contract:5,parents:[4]},
 {unit:6,eclass:1,contract:6,parents:[2]},
]});
assert.equal(m.classes.get(3)[0].unit,4,'prefer a predecessor with higher indegree');
const source=pathTypst(m,m.units.get(4),new Map([[0,m.units.get(5)]]));
assert.ok(source.length<3000,'cycle must remain a reference rather than infinite expansion');
assert.ok(source.includes('cell-unit-4'));
assert.ok(bodyDot({values:[{sort:'E'}],rows:[{op:'A',args:[],result:0}]},[0]).includes('#ffc477'));
console.log('Cell graph ranking, cycles and coloring passed');
const valued=model({nodes:[{unit:0,eclass:0,contract:0,parents:[]}],reuse_value:{cells:[{eclass:0,interface_instances:4}]}});
assert.equal(valued.reuse.get(0).interface_instances,4);
assert.ok(overviewDot(valued).includes('4 interface bindings'));
assert.ok(!overviewDot(m).includes('undefined'));
// History and conditional interfaces are different graphs. Do not replace a
// certified parent with another orientation from the same eclass.
const routed=model({nodes:[
 {unit:0,eclass:0,contract:0,parents:[]},
 {unit:1,eclass:1,contract:1,parents:[0],producer_parents:[0]},
 {unit:2,eclass:2,contract:2,parents:[],history_windows:[{new_entry:true}]},
 {unit:3,eclass:3,contract:3,parents:[0,1],producer_parents:[0],context_parents:[1]},
 {unit:4,eclass:0,contract:4,parents:[]},
],graft_interfaces:[{target:2,parents:[0],proof:{kind:'positive-replay-extension/v1'}}]});
assert.deepEqual(routed.parents(routed.units.get(2)),[0]);
assert.deepEqual(routed.parents(routed.units.get(3)),[],'historical context is not a certified edge');
const route=pathTypst(routed,routed.units.get(2),new Map([[0,routed.units.get(4)]]));
assert.ok(route.includes('cell-unit-0'));
assert.ok(!route.includes('cell-unit-4'),'equal class does not transport binding automatically');
assert.ok(overviewDot(routed).includes('context history'));
assert.ok(overviewDot(routed).includes('#cell-interface-2-0'));
routed.mode='history';assert.deepEqual(routed.parents(routed.units.get(3)),[0,1]);
console.log('Conditional routes, history distinction and exact parent wiring passed');
const family=model({nodes:[{unit:0,eclass:0,contract:0,parents:[]}],repetition_families:{families:[{id:2,units:[0,99],occurrences:[{}],rules:['step']}]}});
assert.equal(family.families.size,1);
assert.ok(overviewDot(family).includes('#cell-family-2'));
assert.ok(overviewDot(family).includes('constraint=false'));
assert.ok(!overviewDot(family).includes('"u99"'),'unregistered unit cannot become a Cell node');
assert.equal(family.classes.size,1,'family annotations never merge Cells');
assert.ok(familyDot(family,2).includes('#cell-family-2'));
console.log('Analyze-only observed families annotate existing Cell nodes');
