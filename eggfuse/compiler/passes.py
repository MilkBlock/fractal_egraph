"""Content-addressed pass receipts, with declared dependencies and validators."""
import hashlib,json,os,shutil,time
from pathlib import Path

def digest(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def fingerprint(value):return hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',',':')).encode()).hexdigest()

def verified_receipts(output, required):
    """Read completed compiler passes without compiling or inventing a fallback.

    Historical tool binaries need not remain installed. Data dependencies and
    emitted artifacts must still match; original implementation hashes are kept
    in the receipts as provenance, not mistaken for the current compiler.
    """
    root=Path(output).resolve()
    if not (root/'pipeline.json').is_file():raise ValueError('expected an eggfuse compile output directory, not a frozen source_core/program')
    pipeline=json.loads((root/'pipeline.json').read_text())
    if pipeline.get('schema')!='eggfuse-passes/v1' or pipeline.get('failed'):raise ValueError('compiler pipeline is incomplete or failed')
    records={}
    for step in pipeline['passes']:
        name=step['name']
        if name in records:raise ValueError('duplicate compiler pass '+name)
        if name not in required:continue
        folder=Path(step['directory']).resolve()
        if folder!=root/'passes'/name/step['key']:raise ValueError('compiler receipt directory differs')
        receipt=json.loads((folder/'receipt.json').read_text())
        contract=receipt['contract']
        if not step.get('validated') or not receipt.get('validated') or contract['name']!=name or fingerprint(contract)!=step['key']:raise ValueError('unvalidated compiler receipt '+name)
        for rel,checksum in receipt['artifacts'].items():
            path=(folder/rel).resolve()
            if not path.is_relative_to(folder) or not path.is_file() or digest(path)!=checksum:raise ValueError('compiler artifact differs: '+name+'/'+rel)
        for item in contract['inputs'].values():
            path=Path(item['path']).resolve()
            if path.is_relative_to(root) and (not path.is_file() or digest(path)!=item['sha256']):raise ValueError('compiler data dependency differs: '+str(path))
        records[name]=(folder,receipt)
    missing=set(required)-records.keys()
    if missing:raise ValueError('missing compiler passes: '+', '.join(sorted(missing)))
    return records

class PassManager:
    def __init__(self,output,stop_after=None):
        self.output=Path(output).resolve()
        if self.output.exists() and any(self.output.iterdir()):
            owner=self.output/'pipeline.json'
            if not owner.is_file() or json.loads(owner.read_text()).get('schema')!='eggfuse-passes/v1':raise ValueError('output is not an owned eggfuse pipeline directory')
        self.output.mkdir(parents=True,exist_ok=True)
        self.stop_after=stop_after;self.receipts=[];self.failed=None
        if not (self.output/'pipeline.json').exists():self.save()
    def save(self):
        p=self.output/'pipeline.json';tmp=p.with_suffix('.pending')
        tmp.write_text(json.dumps(dict(schema='eggfuse-passes/v1',passes=self.receipts,failed=self.failed),indent=2)+'\n');tmp.replace(p)
    def run(self,name,inputs,options,implementation,produces,action,verify,preserves=()):
        dependencies={k:dict(path=str(Path(v).absolute()),sha256=digest(v)) for k,v in inputs.items()}
        code={str(Path(p).resolve()):digest(p) for p in implementation}
        contract=dict(name=name,inputs=dependencies,options=options,implementation=code)
        key=fingerprint(contract);folder=self.output/'passes'/name/key
        receipt_path=folder/'receipt.json';cached=False
        def outputs():
            return sorted({str(p.relative_to(folder)) for pattern in produces for p in (folder.glob(pattern) if '*' in pattern else [folder/pattern])})
        try:
            if receipt_path.exists():
                try:
                    old=json.loads(receipt_path.read_text())
                    cached=old['contract']==contract and set(outputs())==set(old['artifacts']) and all((folder/n).is_file() and digest(folder/n)==h for n,h in old['artifacts'].items())
                except (ValueError,KeyError,OSError):cached=False
                if cached:verify(folder)
            if not cached:
                if folder.exists():shutil.rmtree(folder)
                folder.mkdir(parents=True)
                start=time.perf_counter();action(folder);verify(folder)
                hashes={n:digest(folder/n) for n in outputs()}
                old=dict(contract=contract,artifacts=hashes,seconds=time.perf_counter()-start,validated=True)
                receipt_path.write_text(json.dumps(old,indent=2)+'\n')
            self.receipts.append(dict(name=name,key=key,cached=cached,directory=str(folder),
                seconds=0 if cached else old['seconds'],original_seconds=old['seconds'],
                requires=list(inputs),produces=list(produces),preserves=list(preserves),validated=True))
            self.save();return folder
        except Exception as e:
            self.failed=dict(pass_name=name,directory=str(folder),message=str(e));self.save();raise
    def publish(self,source,name):
        dst=self.output/name;dst.parent.mkdir(parents=True,exist_ok=True)
        temporary=dst.with_name(dst.name+'.pending');shutil.copyfile(source,temporary);temporary.replace(dst)
        return dst
