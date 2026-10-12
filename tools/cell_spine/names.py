"""Generated identifiers: Op! is ingress, Op!! is canonical storage."""


def raw(name):return name+'!'

def canonical(name):return name+'!!'

def summary(family):return f'Summary-{family}!'

def is_summary(name):
    return ((name.startswith('Summary-') and name.endswith('!') and name[8:-1].isdigit()) or
            (name.startswith('Spine-Summary-') and name[14:].isdigit()))

def family(name):
    if not is_summary(name):raise ValueError(f'not a Cell marker: {name}')
    return int(name[14:] if name.startswith('Spine-Summary-') else name[8:-1])

def is_canonical(name):return name.endswith('!!') or name.startswith('Spine-Cano-')

def is_raw(name,signatures=None):
    if signatures is None and name.startswith('Spine-'):
        return not name.startswith(('Spine-Cano-','Spine-Summary-','Spine-Reified-'))
    return (name.endswith('!') and not name.endswith('!!') and
            (name[:-1] in signatures if signatures is not None else
             not is_summary(name) and not name.startswith('Reified-')))
