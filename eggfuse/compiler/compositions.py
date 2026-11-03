"""Backend-independent observed multi-rule recipe pass."""
import json
from pathlib import Path
from eggfuse.analyzer.compositions import build,validate
ROOT=Path(__file__).resolve().parents[2]

def compile_compositions(pm,recipe,marker,evidence):
    def emit(folder):
        r=json.loads(recipe.read_text());m=json.loads(marker.read_text());e=json.loads(evidence.read_text())
        plan=build(r,[x['name'] for x in m['markers']],e)
        (folder/'composition-program.json').write_text(json.dumps(plan,indent=2))
    def check(folder):
        validate(json.loads((folder/'composition-program.json').read_text()),json.loads(recipe.read_text()),
                 [x['name'] for x in json.loads(marker.read_text())['markers']])
    folder=pm.run('compositions',{'recipe':recipe,'marker':marker,'evidence':evidence},{'selection':'observed edges; exact typed proofs'},
                  [Path(__file__),ROOT/'eggfuse/analyzer/compositions.py',ROOT/'eggfuse/analyzer/multistage.py'],
                  ['composition-program.json'],emit,check,preserves=['all original queries, effects, and Run boundaries'])
    return pm.publish(folder/'composition-program.json','composition-program.json')
