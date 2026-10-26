"""Native snapshot-difference adapter for validation only.

The resident compiler consumes row/alias events. This adapter constructs those
events from saved snapshots, not predicate answers. Production commit-journal
wiring is separate; its cost must not be confused with delta compilation.
"""
import json

def row_key(row):
    return row['table'],json.dumps(row['values'][:-1],sort_keys=True,separators=(',',':'))

class SnapshotEvents:
    def __init__(self,templates,rows):
        self.tables=set(templates.by_table);self.current={};self.next_id=len(rows)
        for i,r in enumerate(rows):
            if r['table'] in self.tables:self.current[row_key(r)]=(i,r)

    def diff(self,snapshot):
        incoming={row_key(r):r for r in snapshot['rows'] if r['table'] in self.tables}
        removes=[owner for key,(owner,_) in self.current.items() if key not in incoming]
        upserts=[];next_rows={}
        for key,row in incoming.items():
            if key in self.current:
                owner,old=self.current[key]
                if old!=row:upserts.append((owner,row))
            else:owner=self.next_id;self.next_id+=1;upserts.append((owner,row))
            next_rows[key]=(owner,row)
        self.current=next_rows
        aliases=[(a['sort'],a['old'],a['now']) for a in snapshot['anchors'] if a['old']!=a['now']]
        return {'upserts':upserts,'removes':removes,'aliases':aliases}
