import json,sys
from pathlib import Path
from tools.cell_spine.pass3_markers import compile_source
def main():
    r=json.loads(Path(sys.argv[1]).read_text())
    compile_source(Path(r['source']),Path(r['analysis']),Path(r['output']),Path(r['ast_binary']),
                   json.loads(Path(r['profile']).read_text()) if r['profile'] else None,
                   locality=r.get('locality','identity'))

if __name__ == "__main__":
    main()
