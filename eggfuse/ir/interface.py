"""Lossless typed row interface, independent of the spine experiment DSL."""
def native_row_interface(anchor,ports=()):
    fields=[f'v{i}' for i in range(len(anchor['args']))]
    return {'schema':'typed-marker-interface/v1','owner':'canonical visible source fact',
            'table':anchor['call']['table'],'coordinates':[{'field':v,'column':i,'sort':a['sort']} for i,(v,a) in enumerate(zip(fields,anchor['args']))],
            'root_column':len(fields)-1,'ports':list(ports),
            'correlation':'one complete structural argument tuple per binding row',
            'lifetime':'source generation and active owner; unresolved ports remain subscribed'}
