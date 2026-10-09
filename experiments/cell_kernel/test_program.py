#!/usr/bin/env python3
"""Exact shared-DAG tests and architectural counts, never a timing benchmark.

This exercises a standalone scalar prototype. The recurrence has the same
repeated-child Bop/Uop size arithmetic as the fixed pilot, but this file neither
executes egglog nor checks committed native mutations.
"""

import argparse
import json
from pathlib import Path
import random
import unittest

from program import Program


ROOT = Path(__file__).resolve().parents[2]


def size_program(stages=64):
    """Two independent nested chains, sharing only their literal constants."""
    program = Program()
    inputs = {side: program.input(side) for side in ("left", "right")}
    two, cap = program.const(2), program.const(100)
    chains = {}
    for side, current in inputs.items():
        chain = []
        for index in range(1, stages + 1):
            doubled = program.mul_const(2, current)
            candidate = program.add(doubled, two)
            size = program.minimum(cap, candidate)
            ports = dict(doubled=doubled, candidate=candidate, size=size)
            program.name_cell(f"{side}.stage{index}", ports)
            chain.append(ports)
            current = size
        chains[side] = chain
    outputs = {side: chain[-1]["size"] for side, chain in chains.items()}
    return program, inputs, cap, chains, outputs


def reference_sizes(value, stages):
    result = []
    for _ in range(stages):
        value = min(100, 2 * value + 2)
        result.append(value)
    return result


def architecture_demo():
    """Return reproducible counts with an explicit warm/cold cache comparison."""
    program, inputs, cap, chains, outputs = size_program()
    initial_node_count = len(program.nodes)
    initial_edges = sum(len(node.args) for node in program.nodes)
    depths = []
    for node in program.nodes:
        depths.append(0 if not node.args else 1 + max(depths[i] for i in node.args))

    runtime = program.instance({"left": 60, "right": 1})
    initial_values = runtime.observe(outputs)
    initial_trace = runtime.last_trace
    initial_cache_entries = len(runtime.cache)
    assert initial_values == {"left": 100, "right": 100}
    assert set(runtime.cache) == set(range(initial_node_count))
    assert initial_node_count == 388
    assert initial_edges == 640
    assert max(depths) == 192

    # A named deep port is an existing node reference, not a copied expression.
    deep_port = program.port("left.stage48", "size")
    consumer = program.lt(deep_port, cap)
    warm_value = runtime.observe({"deep_is_below_cap": consumer})
    warm_trace = runtime.last_trace
    assert len(program.nodes) == initial_node_count + 1
    assert program.nodes[consumer].args == (deep_port, cap)
    assert warm_trace["evaluated"] == [consumer]
    assert set(warm_trace["visited"]) == {consumer, deep_port, cap}
    assert warm_value == {"deep_is_below_cap": False}

    # With no retained intermediate cache, that same consumer needs its prefix.
    cold = program.instance({"left": 60, "right": 1})
    cold_value = cold.observe({"deep_is_below_cap": consumer})
    cold_trace = cold.last_trace
    prefix = {node for stage in chains["left"][:48] for node in stage.values()}
    later_left = {node for stage in chains["left"][48:] for node in stage.values()}
    right_nodes = {node for stage in chains["right"] for node in stage.values()}
    assert cold_value == warm_value
    assert set(cold_trace["evaluated"]) == prefix | {cap, program.const(2), consumer}
    assert set(cold_trace["visited"]) == set(cold_trace["evaluated"]) | {inputs["left"]}
    assert not (set(cold_trace["evaluated"]) & (later_left | right_nodes))

    # Changing the first candidate while its minimum remains 100 is a barrier.
    patch_trace = runtime.patch({"left": 61})
    first = chains["left"][0]
    assert patch_trace["recomputed"] == list(first.values())
    assert patch_trace["changed"] == [inputs["left"], first["doubled"], first["candidate"]]
    assert not (set(patch_trace["recomputed"]) & right_nodes)
    assert runtime.observe(outputs) == initial_values
    assert runtime.last_trace["evaluated"] == []

    return {
        "kind": "standalone shared-program architectural checks",
        "native_egglog_executed": False,
        "timing_benchmark": False,
        "recurrence": "s[i+1] = min(100, 2*s[i] + 2)",
        "independent_branches": 2,
        "stages_per_branch": 64,
        "initial_observed_roots": list(outputs),
        "initial_output_values": initial_values,
        "initial_node_count": initial_node_count,
        "initial_edge_count": initial_edges,
        "maximum_dependency_depth": max(depths),
        "cell_alias_count": len(program.cells),
        "initial_evaluated_nodes": initial_trace["counts"]["evaluated"],
        "initial_traversed_nodes_including_cached_boundaries": initial_trace["counts"]["visited"],
        "initial_retained_cache_entries_including_inputs": initial_cache_entries,
        "late_consumer": {
            "expression": "left.stage48.size < 100",
            "new_nodes": len(program.nodes) - initial_node_count,
            "value": warm_value["deep_is_below_cap"],
            "warm_evaluated_nodes": warm_trace["counts"]["evaluated"],
            "warm_traversed_nodes_including_cached_boundaries": warm_trace["counts"]["visited"],
            "warm_retained_cache_entries_including_inputs": len(runtime.cache),
            "cold_evaluated_nodes": cold_trace["counts"]["evaluated"],
            "cold_traversed_nodes_including_cached_boundaries": cold_trace["counts"]["visited"],
            "cold_retained_cache_entries_including_inputs": len(cold.cache),
            "cold_untouched_later_left_stages": 16,
            "cold_untouched_right_stages": 64,
        },
        "min_barrier_patch": {
            "updates": {"left": 61},
            "recomputed_nodes": patch_trace["counts"]["recomputed"],
            "changed_nodes_including_input": patch_trace["counts"]["changed"],
            "unchanged_first_minimum": 100,
            "later_left_stages_recomputed": 0,
            "right_stages_recomputed": 0,
            "late_consumer_recomputed": consumer in patch_trace["recomputed"],
        },
        "storage_contract": "One cache entry per evaluated scalar node, shared by all cell aliases; all input bindings are also retained.",
        "scope": "Counts describe scalar evaluation and retained values, not elapsed time, bytes, pattern discovery, or committed egglog mutations.",
    }


class ProgramTests(unittest.TestCase):
    def test_structural_sharing_evaluates_repeated_node_once(self):
        program = Program()
        x = program.input("x")
        one = program.const(1)
        shared = program.add(x, one)
        self.assertEqual(shared, program.add(x, one))
        self.assertEqual(x, program.input("x"))
        self.assertEqual(one, program.const(1))
        total = program.add(shared, shared)
        runtime = program.instance({"x": 3})
        self.assertEqual(runtime.observe({"shared": shared, "total": total}), {"shared": 4, "total": 8})
        self.assertEqual(runtime.last_trace["evaluated"], [one, shared, total])
        trace = runtime.patch({"x": 4})
        self.assertEqual(trace["recomputed"], [shared, total])
        self.assertEqual(runtime.observe({"total": total}), {"total": 10})
        self.assertEqual(runtime.last_trace["evaluated"], [])

    def test_deep_late_consumer_warm_cold_and_min_barrier(self):
        report = architecture_demo()
        self.assertEqual(report["initial_evaluated_nodes"], 386)
        self.assertEqual(report["late_consumer"]["warm_evaluated_nodes"], 1)
        self.assertEqual(report["late_consumer"]["warm_traversed_nodes_including_cached_boundaries"], 3)
        self.assertEqual(report["late_consumer"]["cold_evaluated_nodes"], 147)
        self.assertEqual(report["late_consumer"]["cold_traversed_nodes_including_cached_boundaries"], 148)
        self.assertEqual(report["min_barrier_patch"]["recomputed_nodes"], 3)

    def test_nested_cells_keep_parent_references_without_flattening(self):
        program, inputs, _, chains, _ = size_program()
        for side, chain in chains.items():
            previous = inputs[side]
            for index, stage in enumerate(chain, 1):
                self.assertEqual(program.nodes[stage["doubled"]].args, (previous,))
                self.assertEqual(program.port(f"{side}.stage{index}", "size"), stage["size"])
                previous = stage["size"]
        self.assertEqual(len(program.nodes), 388)
        self.assertLessEqual(max(len(node.args) for node in program.nodes), 2)

    def test_atomic_batches_match_fresh_full_evaluation_and_reference(self):
        program, _, _, chains, _ = size_program(stages=12)
        outputs = {f"{side}.{i}": stage["size"]
                   for side, chain in chains.items() for i, stage in enumerate(chain)}
        state = {"left": -4, "right": 3}
        runtime = program.instance(state)
        runtime.observe(outputs)
        generator = random.Random(731)
        for _ in range(80):
            updates = {side: generator.randrange(-12, 65) for side in state
                       if generator.choice((False, True))}
            state.update(updates)
            trace = runtime.patch(updates)
            self.assertEqual(len(trace["recomputed"]), len(set(trace["recomputed"])))
            expected = {f"{side}.{i}": value for side, seed in state.items()
                        for i, value in enumerate(reference_sizes(seed, 12))}
            fresh = program.instance(state).observe(outputs)
            self.assertEqual(fresh, expected)
            self.assertEqual(runtime.observe(outputs), expected)
            self.assertEqual(runtime.last_trace["evaluated"], [])

    def test_atomic_cancelling_batch_stops_at_unchanged_sum(self):
        program = Program()
        a, b = program.input("a"), program.input("b")
        total = program.add(a, b)
        downstream = program.mul_const(7, total)
        runtime = program.instance({"a": 3, "b": 4})
        runtime.observe({"value": downstream})
        trace = runtime.patch({"a": 4, "b": 3})
        self.assertEqual(trace["recomputed"], [total])
        self.assertEqual(trace["changed"], [a, b])
        self.assertEqual(runtime.observe({"value": downstream}), {"value": 49})

    def test_unrelated_cached_branch_is_untouched(self):
        program = Program()
        a, b = program.input("a"), program.input("b")
        left, right = program.mul_const(2, a), program.mul_const(3, b)
        runtime = program.instance({"a": 4, "b": 5})
        runtime.observe({"left": left, "right": right})
        right_before = runtime.cache[right]
        trace = runtime.patch({"a": 7})
        self.assertEqual(trace["recomputed"], [left])
        self.assertNotIn(right, trace["changed"])
        self.assertEqual(runtime.cache[right], right_before)

    def test_uncached_input_update_is_seen_by_new_consumer(self):
        program = Program()
        a, b = program.input("a"), program.input("b")
        left, unseen = program.mul_const(3, a), program.mul_const(2, b)
        runtime = program.instance({"a": 4, "b": 1})
        runtime.observe({"left": left})
        trace = runtime.patch({"b": 9})
        self.assertEqual(trace["recomputed"], [])
        self.assertEqual(trace["changed"], [b])
        self.assertNotIn(unseen, runtime.cache)
        threshold = program.const(10)
        new_consumer = program.lt(unseen, threshold)
        self.assertEqual(runtime.observe({"late": new_consumer}), {"late": False})
        self.assertEqual(runtime.last_trace["evaluated"], [unseen, threshold, new_consumer])
        self.assertEqual(runtime.cache[unseen], 18)

    def test_many_cell_aliases_share_one_runtime_cache(self):
        program = Program()
        source = program.input("source")
        shared = program.mul_const(9, source)
        for i in range(512):
            program.name_cell(f"alias{i}", {"size": shared})
        self.assertEqual(len(program.nodes), 2)
        outputs = {f"alias{i}": program.port(f"alias{i}", "size") for i in range(512)}
        runtime = program.instance({"source": 2})
        self.assertEqual(set(runtime.observe(outputs).values()), {18})
        self.assertEqual(runtime.last_trace["evaluated"], [shared])
        self.assertEqual(len(runtime.cache), 2)
        self.assertEqual(runtime.patch({"source": 3})["recomputed"], [shared])
        self.assertEqual(set(runtime.observe(outputs).values()), {27})
        self.assertEqual(runtime.last_trace["evaluated"], [])

    def test_invalid_observation_and_unbound_new_input_leave_cache_unchanged(self):
        program = Program()
        x = program.input("x")
        value = program.mul_const(2, x)
        runtime = program.instance({"x": 3})
        before = dict(runtime.cache)
        for invalid in (-1, len(program.nodes), True):
            with self.assertRaises(ValueError):
                runtime.observe({"valid": value, "invalid": invalid})
            self.assertEqual(runtime.cache, before)
        y = program.input("new_input")
        late = program.add(value, y)
        with self.assertRaises(ValueError):
            runtime.observe({"valid": value, "unbound": late})
        self.assertEqual(runtime.cache, before)
        runtime.patch({"new_input": 4})
        self.assertEqual(runtime.observe({"late": late}), {"late": 10})
        self.assertEqual(runtime.last_trace["evaluated"], [value, late])

    def test_unchanged_and_empty_patch_do_no_work(self):
        program = Program()
        x = program.input("x")
        value = program.mul_const(2, x)
        runtime = program.instance({"x": 10})
        runtime.observe({"value": value})
        before = dict(runtime.cache)
        for patch in ({}, {"x": 10}):
            trace = runtime.patch(patch)
            self.assertEqual(trace["counts"], {"evaluated": 0, "recomputed": 0, "changed": 0})
            self.assertEqual(runtime.cache, before)

    def test_exact_signed_arithmetic_and_boolean_operations(self):
        program = Program()
        x, y = program.input("x"), program.input("y")
        gate, flags = program.input("gate", "bool"), program.input("flags")
        zero = program.const(0)
        total = program.add(x, y)
        equal = program.eq(total, zero)
        negative = program.lt(x, zero)
        bits = program.bit_all(flags, 0b101)
        selected = program.all(gate, equal, negative, bits)
        huge = 2**150
        runtime = program.instance({"x": -huge, "y": huge, "gate": True, "flags": 0b111})
        outputs = {"sum": total, "selected": selected, "zero": program.add(), "true": program.all()}
        self.assertEqual(runtime.observe(outputs), {"sum": 0, "selected": True, "zero": 0, "true": True})
        runtime.patch({"flags": 0b010, "gate": False})
        self.assertEqual(runtime.observe({"selected": selected}), {"selected": False})
        self.assertEqual(program.const(True), program.all())
        self.assertNotEqual(program.const(True), program.const(1))

    def test_types_are_distinct_and_invalid_patch_is_atomic(self):
        program = Program()
        x, gate = program.input("x"), program.input("gate", "bool")
        integer, boolean = program.const(1), program.const(True)
        rejected_constructors = [
            lambda: program.add(x, boolean), lambda: program.minimum(x, gate),
            lambda: program.eq(integer, boolean), lambda: program.lt(gate, boolean),
            lambda: program.all(x), lambda: program.mul_const(True, x),
            lambda: program.bit_all(x, True), lambda: program.const(1.5),
        ]
        for construct in rejected_constructors:
            with self.assertRaises(TypeError):
                construct()
        for bindings in ({"x": True, "gate": True}, {"x": 1, "gate": 1}):
            with self.assertRaises(TypeError):
                program.instance(bindings)
        runtime = program.instance({"x": 1, "gate": True})
        value = program.mul_const(3, x)
        runtime.observe({"value": value})
        before = dict(runtime.cache)
        for patch, error in [({"x": 9, "gate": 0}, TypeError),
                             ({"x": True}, TypeError),
                             ({"x": 9, "missing": 2}, ValueError)]:
            with self.assertRaises(error):
                runtime.patch(patch)
            self.assertEqual(runtime.cache, before)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "out/cell-program-tests/report.json")
    args = parser.parse_args()
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(ProgramTests)
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    if not result.wasSuccessful():
        raise SystemExit(1)
    report = architecture_demo()
    report["unittest_cases_passed"] = result.testsRun
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(f"Architectural checks passed; no timings measured. Report: {args.output}")


if __name__ == "__main__":
    main()
