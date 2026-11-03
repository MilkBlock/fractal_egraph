// Only small view descriptors are retained, never graphs or server jobs.
export class ViewHistory {
  constructor(){this.past=[];this.future=[];this.current=null;this.revision=0;}
  reset(snapshot){this.past=[];this.future=[];this.current=structuredClone(snapshot);this.revision++;}
  updateView(view){if(this.current)this.current.view=structuredClone(view);}
  record(snapshot,label){
    if(!this.current){this.reset(snapshot);return this.revision;}
    if(JSON.stringify(snapshot.config)===JSON.stringify(this.current.config))return this.revision;
    this.past.push({snapshot:this.current,label});this.future=[];
    this.current=structuredClone(snapshot);this.revision++;return this.revision;
  }
  settle(revision,snapshot){
    if(revision!==this.revision)return false;
    this.current=structuredClone(snapshot);return true;
  }
  undo(){
    const entry=this.past.pop();if(!entry)return null;
    this.future.push({snapshot:this.current,label:entry.label});this.current=entry.snapshot;this.revision++;
    return {snapshot:structuredClone(this.current),label:entry.label};
  }
  redo(){
    const entry=this.future.pop();if(!entry)return null;
    this.past.push({snapshot:this.current,label:entry.label});this.current=entry.snapshot;this.revision++;
    return {snapshot:structuredClone(this.current),label:entry.label};
  }
  get undoLabel(){return this.past.at(-1)?.label;}
  get redoLabel(){return this.future.at(-1)?.label;}
}

export function historyShortcut(event){
  if(event.defaultPrevented||event.isComposing||event.altKey||!(event.ctrlKey||event.metaKey))return null;
  const target=event.target;
  if(target?.isContentEditable||target?.closest?.('input, textarea, [role="textbox"], [contenteditable]:not([contenteditable="false"])'))return null;
  const key=event.key.toLowerCase();
  if(key==='z')return event.shiftKey?'redo':'undo';
  if(key==='y'&&event.ctrlKey&&!event.metaKey&&!event.shiftKey)return 'redo';
  return null;
}
