"""Content-addressed pass receipts, with declared dependencies and validators."""
import hashlib,json,os,shutil,time
from pathlib import Path

def digest(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def fingerprint(value):return hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',',':')).encode()).hexdigest()

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
