#include <metal_stdlib>
using namespace metal;
// Dimensions and static program offsets are supplied by the analyze compiler.
constant uint NO = 0xffffffffu;
enum Section {
  META,
  PARENT,
  FACT,
  FH,
  INST,
  IH,
  WATCH,
  WH,
  WS,
  ROWS,
  UNIONS,
  CLEARS,
  EVENTS,
  SEEDS,
  WORK,
  EXT,
  PART,
  PH,
  REF_NEXT,
  INDEX,
  XH,
  OUTPUT,
  NUM,
  NUM_UPDATES,
  NH,
  NS
};
enum Metric {
  NF,
  NI,
  NW,
  NP,
  NE,
  NEED,
  OVERFLOW,
  NOUT,
  WAKES,
  VALIDATIONS,
  LOOKUPS,
  REUSES,
  NX,
  CANDIDATES,
  LAST_EPOCH,
  ERROR,
  EMITTED,
  ANCHORS,
  ACTIVE,
  GUARD_CHECKS,
  GUARD_REJECTS,
  NN
};
struct Params {
  uint off[NS], cap[NS];
  uint epoch, nrows, nunions, nevents, nclears, oldparents, nnums;
};
struct Fact {
  uint table, active, next, spare;
  uint v[MAX_COLS];
};
struct Instance {
  uint t, active;
  atomic_uint dirty;
  uint next;
  uint params[MAX_PARAMS], v[MAX_VALUES];
};
struct Watch {
  uint hash, instance, slot, next;
};
struct WatchSlot {
  uint hash, installed;
};
struct Row {
  uint table, visible, deleted, next;
  uint v[MAX_COLS];
};
struct Event {
  uint rule;
  uint v[MAX_FIELDS];
};
struct Seed {
  uint t, next;
  uint v[MAX_PARAMS];
};
struct External {
  uint fact, part;
};
struct Partial {
  uint part, epoch, delta, next;
  uint v[VARS];
};
struct IndexNode {
  uint row, spec, next;
};
struct Table {
  uint keys, cols, nparts, eq[MAX_COLS], parts[PART_COUNT];
};
struct Read {
  uint table, slots[MAX_COLS];
};
struct Template {
  uint nvals, nparams, nreads, lensStart, nlenses, rule;
  uint paramSlots[MAX_PARAMS], eventFields[MAX_PARAMS], eq[MAX_VALUES],
      initial[MAX_VALUES];
  Read reads[MAX_READS];
};
struct Lens {
  uint part, nalias, slots[VARS], aliases[2 * MAX_ALIASES];
};
struct PartSpec {
  uint table, slots[MAX_COLS], literals[MAX_COLS];
};
struct IndexSpec {
  uint part, nkeys, slots[VARS];
};
struct G {
  device uint *h;
  constant Params *p;
  device const uint *code;
};
#define A(s, t) ((device t *)(g.h + g.p->off[s]))
#define C(o, t) ((device const t *)(g.code + o))
#define CONTEXT                                                                \
  device uint *h [[buffer(0)]], constant Params &p [[buffer(1)]],              \
      device const uint *code [[buffer(2)]]
#define GCTX G g = {h, &p, code}
uint load(G g, uint i) {
  return atomic_load_explicit(A(META, atomic_uint) + i, memory_order_relaxed);
}
void set(G g, uint i, uint v) {
  atomic_store_explicit(A(META, atomic_uint) + i, v, memory_order_relaxed);
}
uint inc(G g, uint i, uint n = 1) {
  return atomic_fetch_add_explicit(A(META, atomic_uint) + i, n,
                                   memory_order_relaxed);
}
uint exchange(device uint *x, uint v) {
  return atomic_exchange_explicit((device atomic_uint *)x, v,
                                  memory_order_relaxed);
}
uint mix(uint v) {
  v ^= v >> 16;
  v *= 0x7feb352du;
  v ^= v >> 15;
  v *= 0x846ca68bu;
  return v ^ (v >> 16);
}
uint addhash(uint h, uint v) { return mix(h ^ mix(v + 0x9e3779b9u)); }
uint canon(G g, uint x) {
  if (x == NO)
    return x;
  for (;;) {
    if (x >= g.p->cap[PARENT]) {
      set(g, ERROR, 1);
      return NO;
    }
    uint y = A(PARENT, uint)[x];
    if (y == x)
      return x;
    x = y;
  }
}
uint vh(uint x) { return addhash(0xa036e917u, x); }
uint facthash(G g, uint table, thread const uint *key) {
  uint h = addhash(0xb489cfe1u, table);
  for (uint c = 0; c < C(TABLE_OFF, Table)[table].keys; c++)
    h = addhash(h, key[c]);
  return h;
}
uint insthash(G g, uint t, thread const uint *key) {
  uint h = addhash(0x912ce8c3u, t);
  for (uint c = 0; c < C(TEMPLATE_OFF, Template)[t].nparams; c++)
    h = addhash(h, key[c]);
  return h;
}
uint parthash(uint part, thread const uint *v) {
  uint h = addhash(0x63173411u, part);
  for (uint c = 0; c < VARS; c++)
    h = addhash(h, v[c]);
  return h;
}
void wake(G g, uint hash) {
  for (uint i = A(WH, uint)[hash & (g.p->cap[WH] - 1)]; i != NO;
       i = A(WATCH, Watch)[i].next) {
    device Watch &w = A(WATCH, Watch)[i];
    if (w.hash != hash)
      continue;
    device WatchSlot &s =
        A(WS, WatchSlot)[w.instance * (MAX_READS + MAX_VALUES) + w.slot];
    if (s.installed && s.hash == hash) {
      atomic_store_explicit(&A(INST, Instance)[w.instance].dirty, 1,
                            memory_order_relaxed);
      inc(g, WAKES);
    }
  }
}
bool watch(G g, uint instance, uint slot, uint hash) {
  device WatchSlot &s =
      A(WS, WatchSlot)[instance * (MAX_READS + MAX_VALUES) + slot];
  if (s.installed && s.hash == hash)
    return true;
  uint i = inc(g, NW);
  if (i >= g.p->cap[WATCH]) {
    set(g, OVERFLOW, 1);
    return false;
  }
  device Watch &w = A(WATCH, Watch)[i];
  w.hash = hash;
  w.instance = instance;
  w.slot = slot;
  w.next = exchange(A(WH, uint) + (hash & (g.p->cap[WH] - 1)), i);
  s.hash = hash;
  s.installed = 1;
  return true;
}
uint lookup(G g, uint t, thread const uint *key) {
  inc(g, LOOKUPS);
  uint hash = facthash(g, t, key), n = C(TABLE_OFF, Table)[t].keys;
  for (uint i = A(FH, uint)[hash & (g.p->cap[FH] - 1)]; i != NO;
       i = A(FACT, Fact)[i].next) {
    device Fact &f = A(FACT, Fact)[i];
    if (f.table != t)
      continue;
    bool equal = true;
    for (uint j = 0; j < n; j++)
      equal &= f.v[j] == key[j];
    if (equal)
      return f.active ? i : NO;
  }
  return NO;
}
// Rehash is only resource maintenance when buffers grow. No facts are inferred.
kernel void init_parents(CONTEXT, uint i [[thread_position_in_grid]]) {
  GCTX;
  i += p.oldparents;
  if (i < p.cap[PARENT])
    A(PARENT, uint)[i] = i;
}
kernel void clear_heads(CONTEXT, uint i [[thread_position_in_grid]]) {
  GCTX;
  if (i < p.cap[FH]) {
    A(FH, uint)[i] = NO;
    A(IH, uint)[i] = NO;
    A(WH, uint)[i] = NO;
    A(PH, uint)[i] = NO;
    A(WORK, uint)[i] = NO;
    A(XH, uint)[i] = NO;
    A(NH, uint)[i] = NO;
  }
}
kernel void rehash(CONTEXT, uint i [[thread_position_in_grid]]) {
  GCTX;
  if (i < load(g, NF)) {
    device Fact &f = A(FACT, Fact)[i];
    uint v[MAX_COLS];
    for (uint c = 0; c < MAX_COLS; c++)
      v[c] = f.v[c];
    uint b = facthash(g, f.table, v) & (p.cap[FH] - 1);
    f.next = exchange(A(FH, uint) + b, i);
  }
  if (i < load(g, NI)) {
    device Instance &x = A(INST, Instance)[i];
    uint v[MAX_PARAMS];
    for (uint c = 0; c < MAX_PARAMS; c++)
      v[c] = x.params[c];
    uint b = insthash(g, x.t, v) & (p.cap[IH] - 1);
    x.next = exchange(A(IH, uint) + b, i);
  }
  if (i < load(g, NW)) {
    device Watch &w = A(WATCH, Watch)[i];
    w.next = exchange(A(WH, uint) + (w.hash & (p.cap[WH] - 1)), i);
  }
  if (i < load(g, NP)) {
    device Partial &r = A(PART, Partial)[i];
    uint v[VARS];
    for (uint c = 0; c < VARS; c++)
      v[c] = r.v[c];
    r.next = exchange(A(PH, uint) + (parthash(r.part, v) & (p.cap[PH] - 1)), i);
  }
  if (i < load(g, NN)) {
    device uint *n = A(NUM, uint) + 4 * i;
    n[3] = exchange(A(NH, uint) + (mix(n[0]) & (p.cap[NH] - 1)), i);
  }
}
kernel void clear_work(CONTEXT, uint i [[thread_position_in_grid]]) {
  GCTX;
  if (i < p.cap[WORK])
    A(WORK, uint)[i] = NO;
}
// Exact signed i64 payloads as two uint32 limbs. Never compare intern IDs,
// cast costs to float, or depend on Metal's optional int64 arithmetic support.
kernel void load_numbers(CONTEXT, uint i [[thread_position_in_grid]]) {
  GCTX;
  if (i >= p.nnums)
    return;
  uint id = A(NUM_UPDATES, uint)[3 * i], index = inc(g, NN);
  if (index >= p.cap[NUM]) {
    set(g, ERROR, 8);
    return;
  }
  device uint *n = A(NUM, uint) + 4 * index;
  n[0] = id;
  n[1] = A(NUM_UPDATES, uint)[3 * i + 1];
  n[2] = A(NUM_UPDATES, uint)[3 * i + 2];
  n[3] = exchange(A(NH, uint) + (mix(id) & (p.cap[NH] - 1)), index);
}
kernel void apply_unions(CONTEXT, uint i [[thread_position_in_grid]]) {
  GCTX;
  if (i >= p.nunions)
    return;
  uint a = A(UNIONS, uint)[2 * i], b = A(UNIONS, uint)[2 * i + 1];
  A(PARENT, uint)[a] = b;
  wake(g, vh(a));
}
kernel void apply_clears(CONTEXT, uint i [[thread_position_in_grid]]) {
  GCTX;
  if (i >= load(g, NF))
    return;
  device Fact &f = A(FACT, Fact)[i];
  if (!f.active)
    return;
  for (uint j = 0; j < p.nclears; j++)
    if (f.table == A(CLEARS, uint)[j]) {
      uint v[MAX_COLS];
      for (uint c = 0; c < MAX_COLS; c++)
        v[c] = f.v[c];
      f.active = 0;
      wake(g, facthash(g, f.table, v));
      if (C(TABLE_OFF, Table)[f.table].nparts)
        set(g, NEED, 1);
    }
}
kernel void row_heads(CONTEXT, uint i [[thread_position_in_grid]]) {
  GCTX;
  if (i >= p.nrows)
    return;
  device Row &r = A(ROWS, Row)[i];
  uint v[MAX_COLS];
  for (uint c = 0; c < MAX_COLS; c++)
    v[c] = r.v[c];
  r.next = exchange(
      A(WORK, uint) + (facthash(g, r.table, v) & (p.cap[WORK] - 1)), i);
}
kernel void apply_rows(CONTEXT, uint b [[thread_position_in_grid]]) {
  GCTX;
  if (b >= p.cap[WORK])
    return;
  for (uint r = A(WORK, uint)[b]; r != NO; r = A(ROWS, Row)[r].next) {
    device Row &e = A(ROWS, Row)[r];
    device const Table &t = C(TABLE_OFF, Table)[e.table];
    uint vals[MAX_COLS];
    for (uint c = 0; c < MAX_COLS; c++)
      vals[c] = e.v[c];
    uint i = A(FH, uint)[b];
    for (; i != NO; i = A(FACT, Fact)[i].next) {
      device Fact &f = A(FACT, Fact)[i];
      if (f.table != e.table)
        continue;
      bool eq = true;
      for (uint c = 0; c < t.keys; c++)
        eq &= f.v[c] == vals[c];
      if (eq)
        break;
    }
    if (i == NO) {
      if (e.deleted)
        continue;
      i = inc(g, NF);
      if (i >= p.cap[FACT]) {
        set(g, ERROR, 2);
        return;
      }
      device Fact &f = A(FACT, Fact)[i];
      f.table = e.table;
      f.active = 0;
      f.next = A(FH, uint)[b];
      A(FH, uint)[b] = i;
      for (uint c = 0; c < MAX_COLS; c++)
        f.v[c] = NO;
      uint first = inc(g, NE, t.nparts);
      if (first + t.nparts > p.cap[EXT]) {
        set(g, ERROR, 3);
        return;
      }
      for (uint c = 0; c < t.nparts; c++) {
        A(EXT, External)[first + c].fact = i;
        A(EXT, External)[first + c].part = t.parts[c];
      }
    }
    device Fact &f = A(FACT, Fact)[i];
    uint active = e.visible && !e.deleted;
    bool changed = f.active != active;
    if (!e.deleted)
      for (uint c = 0; c < t.cols; c++) {
        changed |= f.v[c] != vals[c];
        f.v[c] = vals[c];
      }
    if (changed && (f.active || active)) {
      wake(g, facthash(g, e.table, vals));
      if (t.nparts)
        set(g, NEED, 1);
    }
    f.active = active;
  }
}
kernel void seed_heads(CONTEXT, uint i [[thread_position_in_grid]]) {
  GCTX;
  if (i >= p.nevents * TEMPLATE_COUNT)
    return;
  uint t = i % max(1u, uint(TEMPLATE_COUNT)),
       e = i / max(1u, uint(TEMPLATE_COUNT));
  device const Template &spec = C(TEMPLATE_OFF, Template)[t];
  device Event &ev = A(EVENTS, Event)[e];
  if (ev.rule != spec.rule)
    return;
  device Seed &s = A(SEEDS, Seed)[i];
  s.t = t;
  uint vals[MAX_PARAMS];
  for (uint j = 0; j < spec.nparams; j++)
    vals[j] = s.v[j] = ev.v[spec.eventFields[j]];
  s.next =
      exchange(A(WORK, uint) + (insthash(g, t, vals) & (p.cap[WORK] - 1)), i);
}
kernel void apply_seeds(CONTEXT, uint b [[thread_position_in_grid]]) {
  GCTX;
  if (b >= p.cap[WORK])
    return;
  for (uint e = A(WORK, uint)[b]; e != NO; e = A(SEEDS, Seed)[e].next) {
    device Seed &s = A(SEEDS, Seed)[e];
    uint n = C(TEMPLATE_OFF, Template)[s.t].nparams, i = A(IH, uint)[b];
    for (; i != NO; i = A(INST, Instance)[i].next) {
      device Instance &x = A(INST, Instance)[i];
      if (x.t != s.t)
        continue;
      bool eq = true;
      for (uint j = 0; j < n; j++)
        eq &= x.params[j] == s.v[j];
      if (eq)
        break;
    }
    if (i != NO) {
      inc(g, REUSES);
      continue;
    }
    i = inc(g, NI);
    if (i >= p.cap[INST]) {
      set(g, ERROR, 4);
      return;
    }
    device Instance &x = A(INST, Instance)[i];
    x.t = s.t;
    x.active = 0;
    atomic_store_explicit(&x.dirty, 1, memory_order_relaxed);
    x.next = A(IH, uint)[b];
    A(IH, uint)[b] = i;
    for (uint j = 0; j < n; j++)
      x.params[j] = s.v[j];
    for (uint j = 0; j < MAX_VALUES; j++)
      x.v[j] = NO;
  }
}
kernel void resolve_cells(CONTEXT, uint i [[thread_position_in_grid]]) {
  GCTX;
  if (i >= load(g, NI))
    return;
  device Instance &x = A(INST, Instance)[i];
  if (!atomic_load_explicit(&x.dirty, memory_order_relaxed))
    return;
  inc(g, VALIDATIONS);
  device const Template &t = C(TEMPLATE_OFF, Template)[x.t];
  uint vals[MAX_VALUES];
  bool valid = true, installed = true;
  for (uint c = 0; c < MAX_VALUES; c++)
    vals[c] = t.initial[c];
  for (uint c = 0; c < t.nparams; c++) {
    uint v = x.params[c], s = t.paramSlots[c];
    if (t.eq[s])
      v = canon(g, v);
    vals[s] = v;
  }
  for (uint r = 0; r < t.nreads; r++) {
    device const Read &read = t.reads[r];
    device const Table &table = C(TABLE_OFF, Table)[read.table];
    uint key[MAX_COLS];
    bool ready = true;
    for (uint c = 0; c < table.keys; c++) {
      key[c] = vals[read.slots[c]];
      ready &= key[c] != NO;
    }
    if (!ready) {
      valid = false;
      continue;
    }
    uint hash = facthash(g, read.table, key);
    installed &= watch(g, i, r, hash);
    uint f = lookup(g, read.table, key);
    if (f == NO) {
      valid = false;
      continue;
    }
    uint out = read.slots[table.keys], value = A(FACT, Fact)[f].v[table.keys];
    if (vals[out] != NO && vals[out] != value)
      valid = false;
    else
      vals[out] = value;
  }
  for (uint c = 0; c < t.nvals; c++) {
    if (vals[c] == NO)
      valid = false;
    else if (t.eq[c])
      installed &= watch(g, i, MAX_READS + c, vh(vals[c]));
  }
  bool change = x.active != uint(valid);
  if (valid)
    for (uint c = 0; c < t.nvals; c++)
      change |= x.v[c] != vals[c];
  if (x.active != uint(valid)) {
    if (valid)
      inc(g, ACTIVE);
    else
      atomic_fetch_sub_explicit(A(META, atomic_uint) + ACTIVE, 1,
                                memory_order_relaxed);
  }
  if (change)
    set(g, NEED, 1);
  x.active = valid;
  for (uint c = 0; c < t.nvals; c++)
    x.v[c] = vals[c];
  atomic_store_explicit(&x.dirty, installed ? 0 : 1, memory_order_relaxed);
}
// Fragment projection stays on the device. The CPU never receives these rows.
bool projection(G g, uint ref, thread uint &part, thread uint *v) {
  for (uint c = 0; c < VARS; c++)
    v[c] = NO;
  uint nc = load(g, NI) * MAX_LENSES;
  if (ref < nc) {
    uint i = ref / MAX_LENSES, l = ref % MAX_LENSES;
    device Instance &x = A(INST, Instance)[i];
    device const Template &t = C(TEMPLATE_OFF, Template)[x.t];
    if (!x.active || l >= t.nlenses)
      return false;
    device const Lens &lens = C(LENS_OFF, Lens)[t.lensStart + l];
    part = lens.part;
    for (uint c = 0; c < lens.nalias; c++)
      if (x.v[lens.aliases[2 * c]] != x.v[lens.aliases[2 * c + 1]])
        return false;
    for (uint c = 0; c < VARS; c++)
      if (lens.slots[c] != NO)
        v[c] = x.v[lens.slots[c]];
    return true;
  }
  uint e = ref - nc;
  if (e >= load(g, NE))
    return false;
  device External &r = A(EXT, External)[e];
  device Fact &f = A(FACT, Fact)[r.fact];
  if (!f.active)
    return false;
  part = r.part;
  device const PartSpec &p = C(PART_OFF, PartSpec)[part];
  device const Table &t = C(TABLE_OFF, Table)[f.table];
  for (uint c = 0; c < t.cols; c++) {
    uint val = f.v[c], slot = p.slots[c];
    if (slot == NO) {
      if (p.literals[c] != val)
        return false;
    } else {
      if (v[slot] != NO && v[slot] != val)
        return false;
      v[slot] = val;
    }
  }
  return true;
}
kernel void projection_heads(CONTEXT, uint i [[thread_position_in_grid]]) {
  GCTX;
  if (i >= load(g, NI) * MAX_LENSES + load(g, NE))
    return;
  uint part = NO, v[VARS];
  if (!projection(g, i, part, v))
    return;
  A(REF_NEXT, uint)
  [i] = exchange(A(WORK, uint) + (parthash(part, v) & (p.cap[WORK] - 1)), i);
}
kernel void intern_parts(CONTEXT, uint b [[thread_position_in_grid]]) {
  GCTX;
  if (b >= p.cap[WORK])
    return;
  for (uint r = A(WORK, uint)[b]; r != NO; r = A(REF_NEXT, uint)[r]) {
    uint part = NO, v[VARS];
    if (!projection(g, r, part, v))
      continue;
    uint i = A(PH, uint)[b];
    for (; i != NO; i = A(PART, Partial)[i].next) {
      device Partial &x = A(PART, Partial)[i];
      if (x.part != part)
        continue;
      bool eq = true;
      for (uint c = 0; c < VARS; c++)
        eq &= x.v[c] == v[c];
      if (eq)
        break;
    }
    if (i == NO) {
      i = inc(g, NP);
      if (i >= p.cap[PART]) {
        set(g, ERROR, 5);
        return;
      }
      device Partial &x = A(PART, Partial)[i];
      x.part = part;
      x.epoch = NO;
      x.next = A(PH, uint)[b];
      A(PH, uint)[b] = i;
      for (uint c = 0; c < VARS; c++)
        x.v[c] = v[c];
    }
    device Partial &x = A(PART, Partial)[i];
    if (x.epoch != p.epoch) {
      x.delta = x.epoch != load(g, LAST_EPOCH);
      x.epoch = p.epoch;
    }
  }
}
uint indexhash(G g, uint spec, thread const uint *v) {
  device const IndexSpec &s = C(INDEX_OFF, IndexSpec)[spec];
  uint h = addhash(0x29a95d53u, spec);
  for (uint c = 0; c < s.nkeys; c++)
    h = addhash(h, v[s.slots[c]]);
  return h;
}
kernel void clear_indexes(CONTEXT, uint i [[thread_position_in_grid]]) {
  GCTX;
  if (i < p.cap[XH])
    A(XH, uint)[i] = NO;
}
kernel void build_indexes(CONTEXT, uint i [[thread_position_in_grid]]) {
  GCTX;
  if (i >= load(g, NP))
    return;
  device Partial &r = A(PART, Partial)[i];
  if (r.epoch != p.epoch)
    return;
  uint v[VARS];
  for (uint c = 0; c < VARS; c++)
    v[c] = r.v[c];
  for (uint s = 0; s < INDEX_COUNT; s++)
    if (C(INDEX_OFF, IndexSpec)[s].part == r.part) {
      uint j = inc(g, NX);
      if (j >= p.cap[INDEX]) {
        set(g, ERROR, 6);
        return;
      }
      device IndexNode &n = A(INDEX, IndexNode)[j];
      n.row = i;
      n.spec = s;
      n.next =
          exchange(A(XH, uint) + (indexhash(g, s, v) & (p.cap[XH] - 1)), j);
    }
}
bool i64_operand(G g, device const uint *o, thread const uint *v,
                 thread uint2 &value) {
  if (o[0] == NO) {
    value = uint2(o[1], o[2]);
    return true;
  }
  uint id = v[o[0]];
  for (uint i = A(NH, uint)[mix(id) & (g.p->cap[NH] - 1)]; i != NO;
       i = A(NUM, uint)[4 * i + 3]) {
    device uint *n = A(NUM, uint) + 4 * i;
    if (n[0] == id) {
      value = uint2(n[1], n[2]);
      return true;
    }
  }
  set(g, ERROR, 8);
  return false;
}
bool i64_less(uint2 a, uint2 b) {
  int hi_a = as_type<int>(a.y), hi_b = as_type<int>(b.y);
  return hi_a < hi_b || (hi_a == hi_b && a.x < b.x);
}
bool guards_hold(G g, thread const uint *v) {
  for (uint i = 0; i < GUARD_COUNT; i++) {
    device const uint *o = g.code + GUARD_OFF + 7 * i;
    uint2 a, b;
    if (!i64_operand(g, o + 1, v, a) || !i64_operand(g, o + 4, v, b))
      return false;
    inc(g, GUARD_CHECKS);
    bool lt = i64_less(a, b), gt = i64_less(b, a);
    bool pass = o[0] == 0 ? lt : o[0] == 1 ? gt : o[0] == 2 ? !gt : !lt;
    if (!pass) {
      inc(g, GUARD_REJECTS);
      return false;
    }
  }
  return true;
}
void emit(G g, thread const uint *v) {
  for (uint c = 0; c < VARS; c++)
    if (v[c] == NO) {
      set(g, ERROR, 7);
      return;
    }
  if (!guards_hold(g, v))
    return;
  uint i = inc(g, NOUT);
  if (i >= g.p->cap[OUTPUT]) {
    set(g, OVERFLOW, 2);
    return;
  }
  for (uint c = 0; c < VARS; c++)
    A(OUTPUT, uint)[i * VARS + c] = v[c];
}
// Each lane starts at a changed fragment. Earlier parts must be unchanged:
// this partitions delta matches by their first changed part, without
// duplicates. Index interfaces are baked once, not cached per historical
// execution path.
kernel void match_delta(CONTEXT, uint i [[thread_position_in_grid]]) {
  GCTX;
  if (i >= load(g, NP))
    return;
  device Partial &a = A(PART, Partial)[i];
  if (a.epoch != p.epoch || !a.delta)
    return;
  inc(g, ANCHORS);
  uint frames[PART_COUNT][VARS], cursor[PART_COUNT], started[PART_COUNT];
  for (uint c = 0; c < VARS; c++)
    frames[0][c] = a.v[c];
  for (uint d = 0; d < PART_COUNT; d++)
    started[d] = 0;
  if (PART_COUNT == 1) {
    emit(g, frames[0]);
    return;
  }
  uint anchor = a.part;
  int depth = 0;
  while (depth >= 0) {
    uint spec = g.code[PLAN_OFF + anchor * (PART_COUNT - 1) + uint(depth)];
    if (!started[depth]) {
      cursor[depth] =
          A(XH, uint)[indexhash(g, spec, frames[depth]) & (p.cap[XH] - 1)];
      started[depth] = 1;
    }
    uint n = cursor[depth];
    if (n == NO) {
      started[depth] = 0;
      depth--;
      continue;
    }
    device IndexNode &ix = A(INDEX, IndexNode)[n];
    cursor[depth] = ix.next;
    if (ix.spec != spec)
      continue;
    device Partial &b = A(PART, Partial)[ix.row];
    if (b.part < anchor && b.delta)
      continue;
    inc(g, CANDIDATES);
    bool compatible = true;
    for (uint c = 0; c < VARS; c++)
      if (frames[depth][c] != NO && b.v[c] != NO && frames[depth][c] != b.v[c])
        compatible = false;
    if (!compatible)
      continue;
    for (uint c = 0; c < VARS; c++)
      frames[depth + 1][c] = frames[depth][c] == NO ? b.v[c] : frames[depth][c];
    if (depth == PART_COUNT - 2)
      emit(g, frames[depth + 1]);
    else {
      depth++;
      started[depth] = 0;
    }
  }
}
kernel void finish_query(CONTEXT, uint i [[thread_position_in_grid]]) {
  GCTX;
  if (i == 0) {
    set(g, LAST_EPOCH, p.epoch);
    inc(g, EMITTED, load(g, NOUT));
  }
}
