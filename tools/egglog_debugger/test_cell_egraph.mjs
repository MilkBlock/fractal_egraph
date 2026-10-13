import assert from 'node:assert/strict';
import {model,pathTypst,bodyDot,overviewDot} from './cell-egraph-panel.mjs';
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
