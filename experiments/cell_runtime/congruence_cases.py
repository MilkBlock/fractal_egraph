"""Two correlated constructor chains, followed by a real boundary union."""

def chain(depth=8, instances=1):
    source = '''(datatype E (N i64) (Seed E E) (F E) (Seen E))
(ruleset make) (ruleset alias) (ruleset read)
(rule ((Seed a b)) ('''
    for i in range(depth):
        source += f'(let left{i} (F '+('a' if i == 0 else f'left{i-1}')+'))\n'
        source += f'(let right{i} (F '+('b' if i == 0 else f'right{i-1}')+'))\n'
    source += ''') :ruleset make)
(rule ((Seed a b)) ((union a b)) :ruleset alias)
(rule ((= x (F y))) ((Seen x)) :ruleset read)
'''
    source += ''.join(f'(Seed (N {2*i}) (N {2*i+1}))\n' for i in range(instances))
    return source + '(run-schedule (seq (run make) (run alias) (run read)))\n'

BASE = chain(4)
CASES = {
    'congruence-chain': BASE,
    'congruence-shared-contexts': chain(4, 3),
    'congruence-cross-cell-collision': chain(4,2).replace('(ruleset read)', '''(ruleset read)
(rule ((= a (N 0)) (= b (N 2))) ((union a b)) :ruleset alias)'''),
    'congruence-unseen-interior-alias': BASE.replace('((union a b)) :ruleset alias)', '((union (F a) (F b))) :ruleset alias)'),
    'congruence-preexisting': BASE.replace('(Seed (N 0) (N 1))', '(F (N 0)) (Seed (N 0) (N 1))'),
    'congruence-unready-stage': BASE.replace('(run alias)', '(run read)'),
    'congruence-hidden': BASE.replace('(ruleset read)', '''(ruleset read) (ruleset hide)
(rule ((= a (N 0))) ((subsume (F a))) :ruleset hide)''').replace('(run alias)', '(run hide) (run alias)'),
    'congruence-deleted': BASE.replace('(ruleset read)', '''(ruleset read) (ruleset hide)
(rule ((= a (N 0))) ((delete (F (F (F (F a)))))) :ruleset hide)''').replace('(run alias)', '(run hide) (run alias)'),
    'congruence-custom-merge': BASE.replace('(ruleset make)', '''(function Chosen (E) E :merge new)
(set (Chosen (N 0)) (N 2)) (set (Chosen (N 1)) (N 3))
(ruleset make)'''),
    'congruence-min-merge': BASE.replace('(ruleset make)', '''(function Cost (E) i64 :merge (min old new))
(set (Cost (N 0)) 9) (set (Cost (N 1)) 3)
(ruleset make)'''),
    'congruence-cross-instance': '''(datatype E (N i64) (F E) (G E))
(ruleset make) (ruleset alias)
(rule ((= x (N n))) ((G (F x))) :ruleset make)
(rule ((= a (N 0)) (= b (N 1))) ((union a b)) :ruleset alias)
(N 0) (N 1) (run-schedule (seq (run make) (run alias)))
(check (= (G (F (N 0))) (G (F (N 1)))))''',
}
