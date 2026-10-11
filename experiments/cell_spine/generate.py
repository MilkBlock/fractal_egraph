"""Generate a finite, explicitly whitelisted Add spine demonstration."""
import argparse
from pathlib import Path


def generate(max_size=4):
    if max_size < 3:
        raise ValueError("max_size must be at least three")
    out = [
        '; Original constructors and rules remain unchanged.',
        '(datatype Math (Var String) (Add Math Math))',
        '(ruleset original)',
        '(rewrite (Add a b) (Add b a) :ruleset original)',
        '(rewrite (Add (Add a b) c) (Add a (Add b c)) :ruleset original)',
        '; Shared forked structure: summary identity is outside the structural key.',
        '(datatype Spine (Input Math) (Spine-Add Spine Spine))',
        '(ruleset cell-transition)',
        '(relation Extend (Spine Math))',
        '(relation Export (Spine))',
        '(function Reified (Spine) Math :merge old)',
        '(ruleset export)',
        '(rule ((Export s) (= s (Input x))) ((set (Reified s) x)) :ruleset export)',
        '(rule ((Export s) (= s (Spine-Add a b))) ((Export a) (Export b)) :ruleset export)',
        '(rule ((Export s) (= s (Spine-Add a b)) (= x (Reified a)) (= y (Reified b)))',
        '      ((set (Reified s) (Add x y))) :ruleset export)',
    ]
    for n in range(3, max_size + 1):
        out.append(f'(relation Summary-Add{n} (Spine))')
    for n in range(3, max_size + 1):
        if n < max_size:
            out += [
                f'(rule ((Summary-Add{n} s) (Extend s x))',
                f'      ((Summary-Add{n+1} (Spine-Add s (Input x)))) :ruleset cell-transition)',
            ]
    out += [
        '; Explicit ingress: selected source path, no global pattern search.',
        '(let a (Var "a")) (let b (Var "b")) (let c (Var "c")) (let d (Var "d"))',
        '(let ab (Spine-Add (Input a) (Input b)))',
        '(let abc (Spine-Add ab (Input c)))',
        '(Summary-Add3 abc)',
        '(Extend abc d)',
        '(run cell-transition 4)',
        '(run original 4)',
        '(check (Summary-Add4 (Spine-Add abc (Input d))))' if max_size >= 4
        else '(check (Summary-Add3 abc))',
    ]
    return '\n'.join(out) + '\n'


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--max-size', type=int, default=4)
    parser.add_argument('--output', type=Path, default=Path(__file__).with_name('demo.egg'))
    args = parser.parse_args()
    args.output.write_text(generate(args.max_size))
