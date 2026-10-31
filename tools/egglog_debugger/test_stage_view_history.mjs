import test from 'node:test';
import assert from 'node:assert/strict';
import {ViewHistory,historyShortcut} from './stage-view-history.mjs';

const snapshot=(root,extra={})=>({config:{root,seed:'42',expanded:[],filters:{},...extra},view:{zoom:1,x:0,y:0,selected:null}});

test('undo/redo preserves the exact root, seed, filters and expansion',()=>{
  const h=new ViewHistory();const start=snapshot(14,{filters:{include:['Add']}});h.reset(start);
  const expanded=snapshot(14,{expanded:[14],filters:{include:['Add']}});h.record(expanded,'展开');
  const random=snapshot(null,{seed:'81'});const rev=h.record(random,'换样本');
  h.settle(rev,snapshot(3,{seed:'81'}));
  assert.deepEqual(h.undo().snapshot,expanded);assert.equal(h.undo().snapshot.config.root,14);
  assert.deepEqual(h.redo().snapshot,expanded);assert.equal(h.redo().snapshot.config.root,3);
  assert.equal(h.current.config.seed,'81');
});

test('a new action after undo drops redo; a no-op does not',()=>{
  const h=new ViewHistory();h.reset(snapshot(1));h.record(snapshot(2),'换根');h.undo();
  h.record(snapshot(1),'无变化');assert.equal(h.redoLabel,'换根');
  h.record(snapshot(3),'换样本');assert.equal(h.redo(),null);assert.equal(h.undo().snapshot.config.root,1);
});

test('late responses cannot overwrite an undo or a newer action',()=>{
  const h=new ViewHistory();h.reset(snapshot(1));const old=h.record(snapshot(null),'换样本');
  h.undo();assert.equal(h.settle(old,snapshot(99)),false);assert.equal(h.current.config.root,1);
  const second=h.record(snapshot(2),'展开');const third=h.record(snapshot(3),'换根');
  assert.equal(h.settle(second,snapshot(22)),false);assert.equal(h.settle(third,snapshot(3)),true);
});

test('snapshots are isolated and viewport is restored without adding steps',()=>{
  const h=new ViewHistory();const first=snapshot(1);h.reset(first);first.config.expanded.push(9);
  h.updateView({zoom:2,x:12,y:5,selected:'c1'});h.record(snapshot(2),'换根');
  const undone=h.undo().snapshot;assert.deepEqual(undone.config.expanded,[]);assert.equal(undone.view.zoom,2);
  undone.config.root=99;assert.equal(h.current.config.root,1);
});

test('Ctrl/Cmd Z and redo work; editable fields and IME retain native undo',()=>{
  assert.equal(historyShortcut({key:'z',ctrlKey:true}),'undo');
  assert.equal(historyShortcut({key:'Z',metaKey:true,shiftKey:true}),'redo');
  assert.equal(historyShortcut({key:'y',ctrlKey:true}),'redo');
  for(const extra of [{target:{isContentEditable:true}},{target:{closest:()=>({})}},{isComposing:true},{altKey:true},{defaultPrevented:true}])
    assert.equal(historyShortcut({key:'z',ctrlKey:true,...extra}),null);
  assert.equal(historyShortcut({key:'z'}),null);
});
