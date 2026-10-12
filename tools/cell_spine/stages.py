"""Exact concrete stage specialization, separate from symbolic closure proofs.

This backend does not generalize captured bindings. Its compact runtime carries
an immutable instruction DAG; each stage's full native state is recoverable.
"""
import json
import subprocess
from pathlib import Path


def plan(frames, output, weights=(1.,1.,1.)):
    import gurobipy as gp
    streams = [f['environment'] + f['slot_declarations'] + f['actions'] for f in frames]
    choices = []
    for i, stream in enumerate(streams):
        options = [dict(parent=None, prefix=0, suffix=0, middle=stream)]
        for j in range(i):
            old = streams[j]
            prefix = 0
            while prefix < min(len(old),len(stream)) and old[prefix]==stream[prefix]:
                prefix += 1
            suffix = 0
            while suffix < min(len(old),len(stream))-prefix and old[-suffix-1]==stream[-suffix-1]:
                suffix += 1
            options.append(dict(parent=j, prefix=prefix, suffix=suffix,
                                middle=stream[prefix:len(stream)-suffix if suffix else None]))
        choices.append(options)
    with gp.Model('ordered_stage_recipes') as model:
        model.Params.OutputFlag=0; model.Params.MIPGap=0; model.Params.Threads=1
        variables={}
        for i, options in enumerate(choices):
            for j, choice in enumerate(options):
                variables[i,j]=model.addVar(vtype=gp.GRB.BINARY,name=f'path_{i}_{j}')
            model.addConstr(gp.quicksum(variables[i,j] for j in range(len(options)))==1)
        # A dictionary stores each native command once. The chosen recipes pay
        # for command references and prefix/suffix slices, not engine runtime.
        model.setObjective(gp.quicksum((weights[0]*len(c['middle'])+weights[2]*(2 if c['parent'] is not None else 0))*variables[i,j]
                                      for i,opts in enumerate(choices) for j,c in enumerate(opts)))
        model.write(str(output/'selection.lp')); model.optimize()
        if model.Status != gp.GRB.OPTIMAL:
            raise RuntimeError(f'Gurobi stage selection status {model.Status}')
        selected=[next(c for j,c in enumerate(opts) if variables[i,j].X>.5) for i,opts in enumerate(choices)]
        report=dict(status='optimal',objective=model.ObjVal,bound=model.ObjBound,
                    scope='ordered concrete checkpoint recipe compression; not parameterized coarse-path inference',
                    variables=model.NumVars,constraints=model.NumConstrs)
        model.write(str(output/'selection.sol'))
    # Check lossless reconstruction, including statement order and visibility.
    expanded=[]
    for i,c in enumerate(selected):
        old=expanded[c['parent']] if c['parent'] is not None else []
        got=old[:c['prefix']]+c['middle']+(old[len(old)-c['suffix']:] if c['suffix'] else [])
        if got!=streams[i]: raise AssertionError('stage recipe changed command order')
        expanded.append(got)
    return selected,streams,report


def expand_archive(archive):
    """Lossless ordered decoding; reject invalid indices, slices or cycles."""
    if archive.get('schema')!='ordered-stage-recipes/v1':
        raise ValueError('unsupported recipe schema')
    dictionary=archive['commands']; states=[]
    for i,c in enumerate(archive['recipes']):
        parent=c['parent']
        if parent is not None and (type(parent) is not int or not 0<=parent<i):
            raise ValueError('recipe parent must be an earlier stage')
        base=states[parent] if parent is not None else []
        p,q=c['prefix'],c['suffix']
        if type(p) is not int or type(q) is not int or p<0 or q<0 or p+q>len(base):
            raise ValueError('invalid recipe slice')
        if any(type(k) is not int or not 0<=k<len(dictionary) for k in c['middle']):
            raise ValueError('invalid command reference')
        states.append(base[:p]+[dictionary[k] for k in c['middle']]+(base[-q:] if q else []))
    return states


def compile_stages(source, output, root, reason, weights=(1.,1.,1.)):
    capture=output/'stage-bake'
    subprocess.run([root/'target/release/examples/spine_stages',source,capture],check=True,cwd=root)
    data=json.loads((capture/'stages.json').read_text())
    frames=data['frames']
    selected,streams,selection=plan(frames,output,weights)
    dictionary={}
    for stream in streams:
        for command in stream:
            dictionary.setdefault(command,len(dictionary))
    # The full typed fact graph is in the checked export. This additional DAG
    # shares its restore recipes without turning it into an opaque String blob.
    lines=['; concrete-stage-bake/v1: specialized to the recorded source and input',
           '; This is an ordered recipe spine, not a generalized RipSatCell library.',
           '(datatype StageSpine (StageEmpty) (StageCommand i64 StageSpine)',
           '  (StagePrefix StageSpine i64) (StageSuffix StageSpine i64)',
           '  (StageConcat StageSpine StageSpine))',
           '(relation StageSummary (i64 StageSpine))', '(let recipe_empty (StageEmpty))']
    shared_nodes={}
    for i,c in enumerate(selected):
        expr='recipe_empty'
        for command in reversed(c['middle']):
            key=(dictionary[command],expr)
            if key not in shared_nodes:
                name=f'recipe_node_{len(shared_nodes)}'
                lines.append(f'(let {name} (StageCommand {key[0]} {expr}))')
                shared_nodes[key]=name
            expr=shared_nodes[key]
        if c['parent'] is not None:
            expr=f'(StageConcat (StagePrefix stage_{c["parent"]} {c["prefix"]}) (StageConcat {expr} (StageSuffix stage_{c["parent"]} {c["suffix"]})))'
        lines.extend([f'; original command {frames[i]["command_ordinal"]}: {frames[i]["command"].replace(chr(10)," ")}',
                      f'(let stage_{i} {expr})',f'(StageSummary {i} stage_{i})'])
    runtime='\n'.join(lines)+'\n'
    # Separate scopes prevent expired function rows/union classes/visibility
    # from leaking into the next checkpoint. Checks/extracts keep their stage.
    restore='\n'.join(('(push)\n'+'\n'.join(s)+'\n(pop)') if i+1<len(streams) else '\n'.join(s) for i,s in enumerate(streams))+'\n'
    pending=output/'work'/'stages-runtime.egg';pending.write_text(runtime)
    subprocess.run([root/'target/release/examples/native_math_baseline',pending],check=True,cwd=root,
                   stdout=subprocess.DEVNULL)
    restored=output/'work'/'stages-restored.egg';restored.write_text(restore)
    subprocess.run([root/'target/release/examples/native_math_baseline',restored],check=True,cwd=root,stdout=subprocess.DEVNULL)
    comments='\n'.join('; command '+str(i)+': '+s.replace('\n','\n; ') for i,s in enumerate(dictionary))+'\n'
    (output/'snapshot-dictionary.egg').write_text(comments)
    (output/'snapshot.egg').write_text(runtime)
    (output/'snapshot-restore.egg').write_text(restore)
    archive=json.dumps({
        'schema':'ordered-stage-recipes/v1','commands':list(dictionary),
        'recipes':[dict(parent=c['parent'],prefix=c['prefix'],suffix=c['suffix'],middle=[dictionary[x] for x in c['middle']]) for c in selected]
    },indent=2)+'\n'
    if expand_archive(json.loads(archive))!=streams: raise AssertionError('encoded archive changed stage instructions')
    (output/'snapshot-library.json').write_text(archive)
    (output/'validation.json').write_text(json.dumps({
        'scope':'native stage states and observable outputs; concrete input only',
        'cases':[dict(stage=f['id'],command=f['command'],rows=f['rows'],hidden_rows=f['hidden_rows'],
                      comparison=f['comparison'],original_rules_executed_on_restore=False,outputs=f['output']) for f in frames]
    },indent=2)+'\n')
    return dict(status='archived_stages',cellified=False,representation='concrete_snapshot_archive',
                compressed=(len(archive.encode())+len(runtime.encode()) < sum(len('\n'.join(s).encode()) for s in streams)),generalizes_bindings=False,
                lowering_reason=reason,scope='concrete source/input stage specialization; not a parameterized Cell transition library',
                plan=selection,stages=len(frames),unique_commands=len(dictionary),
                unshared_command_references=sum(map(len,streams)),stored_command_references=sum(len(c['middle']) for c in selected),
                dictionary_bytes=sum(len(s.encode()) for s in dictionary),
                archive_bytes=len(archive.encode())+len(runtime.encode()),
                archive_smaller=(len(archive.encode())+len(runtime.encode()) < sum(len('\n'.join(s).encode()) for s in streams)),
                net_archive_savings_bytes=sum(len('\n'.join(s).encode()) for s in streams)-len(archive.encode())-len(runtime.encode()),
                expanded_recipe_bytes=sum(len('\n'.join(s).encode()) for s in streams),
                snapshot_bytes=len(runtime.encode()),
                compression_baseline='unshared checkpoint restore streams, not the original source or egraph RSS',
                archive_byte_scope='snapshot.egg + snapshot-library.json; diagnostic reports, expanded restore and dictionary comments are excluded',
                original_rules_executed_on_restore=False)
