#!/usr/bin/env python3
"""Exact discovery, cut, and packet checks for the standalone scalar prototype.

The report contains architectural counts only. These tests do not run egglog,
commit native mutations, or measure performance.
"""

import argparse
import copy
import json
from pathlib import Path
import random
import unittest
from unittest.mock import patch

from program import Program
from test_program import reference_sizes, size_program
from touch_backend import TouchRequest
from touch_join import join_packets
from touchpoints import discover


ROOT = Path(__file__).resolve().parents[2]


def three_consumers():
    """Only consumer roots are supplied; no shared ports or cuts are selected."""
    program, inputs, cap, chains, final = size_program(64)
    consumers = {
        "final_compare": {"less": program.lt(final["left"], final["right"])},
        "final_sum": {"sum": program.add(final["left"], final["right"])},
        "stage48_compare": {
            "less": program.lt(chains["left"][47]["size"],
                               chains["right"][47]["size"])
        },
    }
    return program, inputs, cap, chains, consumers


def expected_consumers(bindings):
    """Independent arithmetic reference, with no discovery/runtime calls."""
    left = reference_sizes(bindings["left"], 64)
    right = reference_sizes(bindings["right"], 64)
    return {
        "final_compare": {"less": left[-1] < right[-1]},
        "final_sum": {"sum": left[-1] + right[-1]},
        "stage48_compare": {"less": left[47] < right[47]},
    }


def independent_demands(program, consumers):
    """Walk every consumer separately; do not reuse the discovery algorithm."""
    demands = [set() for _ in program.nodes]
    for consumer, outputs in consumers.items():
        pending, visited = list(outputs.values()), set()
        while pending:
            node = pending.pop()
            if node in visited:
                continue
            visited.add(node)
            demands[node].add(consumer)
            pending.extend(program.nodes[node].args)
    return demands


def correlated_rows():
    """One connected two-output fragment with deliberately crossed row values."""
    program = Program()
    source, delta = program.input("source"), program.input("delta")
    first = program.mul_const(1, source)
    second = program.add(first, delta)
    ports = {"first": first, "second": second}
    plan = discover(program, {"a": ports, "b": ports})
    assert len(plan.fragments) == 1
    runtimes, packets = {}, {}
    for key, first_value, second_value in (("left-a", 3, 4), ("left-b", 30, 40),
                                          ("right-crossed", 3, 40), ("right-exact", 3, 4),
                                          ("left-duplicate", 3, 4)):
        runtime = program.instance({"source": first_value, "delta": second_value - first_value})
        runtimes[key] = runtime
        packets[key] = plan.observe(runtime, key)["packets"][0]
    return program, ports, runtimes, packets


def joint_row_demo():
    _, ports, runtimes, packets = correlated_rows()
    joined = join_packets([packets["left-a"], packets["left-b"]],
                          [packets["right-crossed"], packets["right-exact"]],
                          [(ports["first"], ports["first"]),
                           (ports["second"], ports["second"])], runtimes)
    assert len(joined) == 1
    assert joined[0].left is packets["left-a"]
    assert joined[0].right is packets["right-exact"]
    assert joined[0].key == (3, 4)
    return {
        "left_rows": [[3, 4], [30, 40]],
        "right_rows": [[3, 40], [3, 4]],
        "joint_key_matches": len(joined),
        "matched_instance_pairs": [[match.left.instance_key, match.right.instance_key]
                                   for match in joined],
        "crossed_row_3_40_matched": False,
        "original_packet_witnesses_preserved": True,
    }


def assert_cut_invariants(test, program, consumers, plan):
    """Check ownership, maximal components, and every cut edge independently."""
    demands = independent_demands(program, consumers)
    shared = {i for i, node in enumerate(program.nodes)
              if node.op not in ("input", "const") and len(demands[i]) >= 2}
    roots = {i for outputs in consumers.values() for i in outputs.values()}
    users = [set() for _ in program.nodes]
    for user, node in enumerate(program.nodes):
        for arg in node.args:
            users[arg].add(user)

    owners = {}
    for fragment in plan.fragments:
        members = set(fragment.nodes)
        test.assertTrue(members)
        test.assertEqual(len(members), len(fragment.nodes))
        test.assertEqual(fragment.nodes, tuple(sorted(members)))
        test.assertGreaterEqual(len(fragment.consumers), 2)
        for node in members:
            test.assertNotIn(node, owners, "a shared operator has multiple owners")
            owners[node] = fragment.id
            test.assertEqual(tuple(sorted(demands[node])), fragment.consumers)
            test.assertNotIn(program.nodes[node].op, ("input", "const"))

        expected_inputs = {arg for node in members for arg in program.nodes[node].args
                           if arg not in members}
        expected_outputs = {node for node in members
                            if node in roots or users[node] - members}
        test.assertEqual(fragment.inputs, tuple(sorted(expected_inputs)))
        test.assertEqual(fragment.outputs, tuple(sorted(expected_outputs)))

        # Every fragment is one connected component, even when edges are viewed
        # backwards. Shared constants do not connect independent operator chains.
        frontier, reached = [next(iter(members))], set()
        while frontier:
            node = frontier.pop()
            if node in reached:
                continue
            reached.add(node)
            frontier.extend((set(program.nodes[node].args) | users[node]) & members - reached)
        test.assertEqual(reached, members)

        # Input support is the transitive original input boundary, not just the
        # immediate fragment imports. This detects stale downstream packets.
        pending, support, visited = list(members), set(), set()
        while pending:
            node = pending.pop()
            if node in visited:
                continue
            visited.add(node)
            if program.nodes[node].op == "input":
                support.add(node)
            pending.extend(program.nodes[node].args)
        test.assertEqual(fragment.input_support, tuple(sorted(support)))

    test.assertEqual(set(owners), shared, "every shared operator must be owned once")
    test.assertEqual(len({fragment.id for fragment in plan.fragments}), len(plan.fragments))
    by_id = {fragment.id: fragment for fragment in plan.fragments}
    for user, node in enumerate(program.nodes):
        for arg in node.args:
            if user in shared and arg in shared and demands[user] == demands[arg]:
                test.assertEqual(owners[user], owners[arg], "same-mask neighbors must merge")
            if owners.get(user) == owners.get(arg):
                continue
            if arg in owners:
                test.assertIn(arg, by_id[owners[arg]].outputs, "cross-edge source was omitted")
            if user in owners:
                test.assertIn(arg, by_id[owners[user]].inputs, "cross-edge import was omitted")


def architecture_demo():
    program, _, _, chains, consumers = three_consumers()
    before = copy.deepcopy(program.to_dict())
    plan = discover(program, consumers)
    case = unittest.TestCase()
    assert_cut_invariants(case, program, consumers, plan)
    case.assertEqual(program.to_dict(), before)
    case.assertEqual(len(plan.fragments), 4)
    fragments = []
    for side in ("left", "right"):
        for first, last, names in ((1, 48, tuple(sorted(consumers))),
                                   (49, 64, ("final_compare", "final_sum"))):
            nodes = {node for stage in chains[side][first - 1:last] for node in stage.values()}
            matches = [fragment for fragment in plan.fragments if set(fragment.nodes) == nodes]
            case.assertEqual(len(matches), 1)
            fragment = matches[0]
            case.assertEqual(fragment.consumers, names)
            fragments.append({
                "id": fragment.id,
                "branch": side,
                "stages": [first, last],
                "operator_count": len(fragment.nodes),
                "consumers": list(fragment.consumers),
                "input_node_ids": list(fragment.inputs),
                "output_node_ids": list(fragment.outputs),
                "input_support_node_ids": list(fragment.input_support),
            })
    rows = []
    for key, bindings in (("row-negative", {"left": -3, "right": -2}),
                          ("row-reversed", {"left": -2, "right": -3}),
                          ("row-capped", {"left": 60, "right": 1})):
        runtime = program.instance(bindings)
        observed = plan.observe(runtime, key)
        case.assertEqual(observed["consumers"], expected_consumers(bindings))
        case.assertEqual(len(observed["packets"]), 4)
        case.assertTrue(all(packet.valid(runtime) for packet in observed["packets"]))
        rows.append({"instance_key": key, "inputs": bindings,
                     "consumers": observed["consumers"], "packet_count": len(observed["packets"])})
    return {
        "kind": "standalone touchpoint discovery architectural checks",
        "native_egglog_executed": False,
        "timing_benchmark": False,
        "recurrence": "s[i+1] = min(100, 2*s[i] + 2)",
        "independent_branches": 2,
        "stages_per_branch": 64,
        "consumer_count": len(consumers),
        "node_count_including_inputs_constants_consumers": len(program.nodes),
        "shared_operator_count": sum(len(fragment.nodes) for fragment in plan.fragments),
        "fragment_count": len(plan.fragments),
        "fragments": fragments,
        "exact_rows": rows,
        "joint_row_check": joint_row_demo(),
        "old_dag_unchanged": True,
        "all_cut_edges_preserved": True,
        "shared_operators_owned_exactly_once": True,
        "scope": "Structural sharing in one pure scalar DAG; no arbitrary semantic equivalence search, timings, or committed egglog mutations.",
    }


class TouchpointTests(unittest.TestCase):
    def test_three_consumer_roots_discover_48_and_16_stage_cuts(self):
        report = architecture_demo()
        self.assertEqual(report["shared_operator_count"], 384)
        self.assertEqual(report["fragment_count"], 4)
        self.assertEqual([fragment["operator_count"] for fragment in report["fragments"]],
                         [144, 48, 144, 48])

    def test_discovery_does_not_modify_original_nodes_aliases_or_reverse_edges(self):
        program, _, _, _, consumers = three_consumers()
        before = copy.deepcopy((program.nodes, program.inputs, program.cells,
                                program._interned, program._users))
        plan = discover(program, consumers)
        self.assertEqual((program.nodes, program.inputs, program.cells,
                          program._interned, program._users), before)
        self.assertIs(plan.program, program)
        assert_cut_invariants(self, program, consumers, plan)

    def test_constants_and_inputs_do_not_join_independent_operator_components(self):
        program = Program()
        x, y, one = program.input("x"), program.input("y"), program.const(1)
        left, right = program.add(x, one), program.add(y, one)
        consumers = {"a": {"sum": program.add(left, right)},
                     "b": {"less": program.lt(left, right)}}
        plan = discover(program, consumers)
        self.assertEqual({fragment.nodes for fragment in plan.fragments}, {(left,), (right,)})
        assert_cut_invariants(self, program, consumers, plan)

    def test_exports_include_roots_and_every_cross_edge_even_unrequested_users(self):
        program = Program()
        x, one = program.input("x"), program.const(1)
        first = program.add(x, one)
        final = program.mul_const(2, first)
        latent = program.mul_const(7, first)
        consumers = {"a": {"value": final}, "b": {"value": final}}
        plan = discover(program, consumers)
        self.assertEqual(len(plan.fragments), 1)
        fragment = plan.fragments[0]
        self.assertEqual(fragment.nodes, (first, final))
        self.assertEqual(fragment.outputs, (first, final))
        self.assertNotIn(latent, fragment.nodes)
        assert_cut_invariants(self, program, consumers, plan)
        runtime = program.instance({"x": 3})
        observed = plan.observe(runtime, "row")
        self.assertEqual(observed["consumers"], {"a": {"value": 8}, "b": {"value": 8}})
        self.assertNotIn(latent, runtime.cache)
        self.assertEqual(observed["packets"][0].value(first), 4)

    def test_consumer_definition_order_does_not_change_the_discovered_plan(self):
        program, _, _, _, consumers = three_consumers()
        first = discover(program, consumers)
        reordered = {name: dict(reversed(list(outputs.items())))
                     for name, outputs in reversed(list(consumers.items()))}
        second = discover(program, reordered)
        self.assertEqual(first.fragments, second.fragments)
        self.assertEqual(dict(first.consumers), dict(second.consumers))
        runtime = program.instance({"left": -3, "right": -2})
        self.assertEqual(first.observe(runtime, "row"), second.observe(runtime, "row"))

    def test_rediscovery_with_more_consumers_leaves_previous_plan_unchanged(self):
        program, _, _, chains, final = size_program(64)
        definitions = {"final_compare": {"less": program.lt(final["left"], final["right"])},
                       "final_sum": {"sum": program.add(final["left"], final["right"])}}
        old = discover(program, definitions)
        old_fragments = old.fragments
        old_definitions = {name: dict(outputs) for name, outputs in old.consumers.items()}
        old_nodes = tuple(program.nodes)
        self.assertEqual(sorted(len(fragment.nodes) for fragment in old.fragments), [192, 192])
        runtime = program.instance({"left": -3, "right": -2})
        old_observation = old.observe(runtime, "row")
        definitions["stage48_compare"] = {
            "less": program.lt(chains["left"][47]["size"], chains["right"][47]["size"])}
        new = discover(program, definitions)
        definitions["final_sum"]["sum"] = program.const(123)
        self.assertEqual(program.nodes[:len(old_nodes)], list(old_nodes))
        self.assertEqual(old.fragments, old_fragments)
        self.assertEqual(dict(old.consumers), old_definitions)
        self.assertEqual(len(new.fragments), 4)
        self.assertEqual(old.observe(runtime, "row"), old_observation)
        self.assertEqual(new.observe(runtime, "row")["consumers"],
                         expected_consumers({"left": -3, "right": -2}))
        self.assertEqual(set(old.consumers), {"final_compare", "final_sum"})

    def test_packets_keep_correlated_values_and_expose_only_declared_boundaries(self):
        program, _, _, _, consumers = three_consumers()
        plan = discover(program, consumers)
        rows = (("a", {"left": -3, "right": -2}),
                ("b", {"left": -2, "right": -3}))
        observed_rows = []
        fragments = {fragment.id: fragment for fragment in plan.fragments}
        for key, bindings in rows:
            runtime = program.instance(bindings)
            observed = plan.observe(runtime, key)
            observed_rows.append(observed)
            self.assertEqual(observed["consumers"], expected_consumers(bindings))
            for packet in observed["packets"]:
                fragment = fragments[packet.fragment_id]
                self.assertEqual(packet.instance_key, key)
                self.assertIs(packet.program, program)
                self.assertEqual(packet.inputs, tuple((node, runtime.cache[node]) for node in fragment.inputs))
                self.assertEqual(packet.outputs, tuple((node, runtime.cache[node]) for node in fragment.outputs))
                self.assertEqual(packet.bindings,
                                 tuple((node, bindings[program.nodes[node].value])
                                       for node in fragment.input_support))
                for node in fragment.inputs + fragment.outputs:
                    self.assertEqual(packet.value(node), runtime.cache[node])
                interior = next(node for node in fragment.nodes if node not in fragment.outputs)
                with self.assertRaises(ValueError):
                    packet.value(interior)
        self.assertTrue(observed_rows[0]["consumers"]["final_compare"]["less"])
        self.assertFalse(observed_rows[1]["consumers"]["final_compare"]["less"])
        self.assertEqual(observed_rows[0]["consumers"]["final_sum"]["sum"], -(2 ** 64) - 4)
        self.assertTrue(all(packet.instance_key == "a" for packet in observed_rows[0]["packets"]))

    def test_patches_match_fresh_outputs_and_invalidate_only_dependent_packets(self):
        program, inputs, _, _, consumers = three_consumers()
        program.input("unrelated")
        plan = discover(program, consumers)
        state = {"left": -3, "right": -2, "unrelated": 7}
        runtime = program.instance(state)
        initial = plan.observe(runtime, "row")
        runtime.patch({"left": -4})
        state["left"] = -4
        by_id = {fragment.id: fragment for fragment in plan.fragments}
        for packet in initial["packets"]:
            expected = inputs["left"] not in by_id[packet.fragment_id].input_support
            self.assertEqual(packet.valid(runtime), expected)
        updated = plan.observe(runtime, "row")
        self.assertEqual(updated["consumers"], expected_consumers(state))
        self.assertEqual(updated["consumers"], plan.observe(program.instance(state), "fresh")["consumers"])
        self.assertTrue(all(packet.valid(runtime) for packet in updated["packets"]))
        runtime.patch({"unrelated": 99})
        self.assertTrue(all(packet.valid(runtime) for packet in updated["packets"]))
        self.assertEqual(plan.observe(runtime, "row"), updated)
        self.assertEqual(runtime.last_trace["evaluated"], [])

    def test_random_atomic_patches_agree_with_fresh_and_independent_arithmetic(self):
        program, _, _, _, consumers = three_consumers()
        plan = discover(program, consumers)
        state = {"left": -3, "right": 1}
        runtime = program.instance(state)
        generator = random.Random(48264)
        for _ in range(24):
            updates = {name: generator.randrange(-8, 65) for name in state
                       if generator.choice((True, False))}
            state.update(updates)
            runtime.patch(updates)
            observed = plan.observe(runtime, "row")
            self.assertEqual(observed["consumers"], expected_consumers(state))
            self.assertEqual(observed["consumers"],
                             plan.observe(program.instance(state), "fresh")["consumers"])
            self.assertTrue(all(packet.valid(runtime) for packet in observed["packets"]))

    def test_packet_validity_is_conservative_when_minimum_blocks_value_changes(self):
        program, inputs, _, _, consumers = three_consumers()
        plan = discover(program, consumers)
        runtime = program.instance({"left": 60, "right": 1})
        before = plan.observe(runtime, "row")
        runtime.patch({"left": 61})
        self.assertEqual(plan.observe(runtime, "row")["consumers"], before["consumers"])
        for fragment, packet in zip(plan.fragments, before["packets"]):
            self.assertEqual(packet.valid(runtime), inputs["left"] not in fragment.input_support)

    def test_empty_and_single_consumer_have_no_shared_fragments(self):
        program = Program()
        x = program.input("x")
        value = program.mul_const(3, x)
        runtime = program.instance({"x": 4})
        empty = discover(program, {})
        self.assertEqual(empty.fragments, ())
        self.assertEqual(empty.observe(runtime, "row"), {"consumers": {}, "packets": ()})
        single = discover(program, {"only": {"value": value}})
        self.assertEqual(single.fragments, ())
        self.assertEqual(single.observe(runtime, "row"),
                         {"consumers": {"only": {"value": 12}}, "packets": ()})

    def test_multiple_aliases_in_one_consumer_do_not_create_shared_fragments(self):
        program = Program()
        x = program.input("x")
        value = program.mul_const(3, x)
        plan = discover(program, {"one_consumer": {"a": value, "b": value, "c": value}})
        self.assertEqual(plan.fragments, ())

    def test_joint_key_join_rejects_crossed_marginal_values(self):
        report = joint_row_demo()
        self.assertEqual(report["joint_key_matches"], 1)
        self.assertEqual(report["matched_instance_pairs"], [["left-a", "right-exact"]])

    def test_join_preserves_distinct_instance_witnesses_with_identical_values(self):
        _, ports, runtimes, packets = correlated_rows()
        left = [packets["left-a"], packets["left-duplicate"]]
        right = [packets["right-exact"]]
        links = [(ports["first"], ports["first"]), (ports["second"], ports["second"])]
        joined = join_packets(left, right, links, runtimes)
        self.assertEqual(len(joined), 2)
        for match, original in zip(joined, left):
            self.assertIs(match.left, original)
            self.assertIs(match.right, packets["right-exact"])
            self.assertEqual(match.key, (3, 4))

    def test_join_rejects_stale_rows_including_rows_without_a_match(self):
        _, ports, runtimes, packets = correlated_rows()
        links = [(ports["first"], ports["first"]), (ports["second"], ports["second"])]
        runtimes["left-b"].patch({"source": 31})
        with self.assertRaises(ValueError):
            join_packets([packets["left-a"], packets["left-b"]],
                         [packets["right-exact"]], links, runtimes)
        with self.assertRaises(ValueError):
            join_packets([packets["left-b"]], [], links, runtimes)
        runtimes.pop("right-exact")
        with self.assertRaises(ValueError):
            join_packets([packets["left-a"]], [packets["right-exact"]], links, runtimes)

    def test_join_rejects_unexposed_ports_and_implicit_cartesian_product(self):
        program, ports, runtimes, packets = correlated_rows()
        left, right = [packets["left-a"]], [packets["right-exact"]]
        with self.assertRaises(ValueError):
            join_packets(left, right, [], runtimes)
        unavailable = program.const(999)
        with self.assertRaises(ValueError):
            join_packets(left, right, [(unavailable, ports["first"])], runtimes)
        with self.assertRaises(ValueError):
            join_packets(left, [], [(True, ports["first"])], runtimes)

    def test_join_checks_declared_types_instead_of_python_bool_integer_equality(self):
        integer = Program()
        source = integer.input("source")
        value = integer.mul_const(1, source)
        integer_plan = discover(integer, {"a": {"value": value}, "b": {"value": value}})
        integer_runtime = integer.instance({"source": 1})
        integer_packet = integer_plan.observe(integer_runtime, "integer")["packets"][0]
        boolean = Program()
        gate = boolean.input("gate", "bool")
        flag = boolean.all(gate, boolean.const(True))
        boolean_plan = discover(boolean, {"a": {"flag": flag}, "b": {"flag": flag}})
        boolean_runtime = boolean.instance({"gate": True})
        boolean_packet = boolean_plan.observe(boolean_runtime, "boolean")["packets"][0]
        self.assertEqual(integer_packet.value(value), boolean_packet.value(flag))
        with self.assertRaises(TypeError):
            join_packets([integer_packet], [boolean_packet], [(value, flag)],
                         {"integer": integer_runtime, "boolean": boolean_runtime})

    def test_join_requires_one_program_namespace_per_side(self):
        _, ports, runtimes, packets = correlated_rows()
        other = Program()
        source, delta = other.input("source"), other.input("delta")
        first = other.mul_const(1, source)
        second = other.add(first, delta)
        plan = discover(other, {"a": {"first": first, "second": second},
                                "b": {"first": first, "second": second}})
        foreign_runtime = other.instance({"source": 3, "delta": 1})
        foreign_packet = plan.observe(foreign_runtime, "foreign")["packets"][0]
        runtimes["foreign"] = foreign_runtime
        links = [(first, ports["first"]), (second, ports["second"])]
        # Explicit links may relate two different Programs, provided each side
        # has one unambiguous node-ID namespace.
        self.assertEqual(len(join_packets([foreign_packet], [packets["right-exact"]],
                                          links, runtimes)), 1)
        with self.assertRaises(ValueError):
            join_packets([packets["left-a"], foreign_packet],
                         [packets["right-exact"]], links, runtimes)
        with self.assertRaises(ValueError):
            join_packets([packets["left-a"]], [packets["right-exact"], foreign_packet],
                         links, runtimes)

    def test_joined_witnesses_feed_a_new_consumer_and_recheck_freshness(self):
        _, ports, runtimes, packets = correlated_rows()
        joined = join_packets([packets["left-a"]], [packets["right-crossed"]],
                              [(ports["first"], ports["first"])], runtimes)
        self.assertEqual(len(joined), 1)
        consumer = Program()
        left, right = consumer.input("left"), consumer.input("right")
        outputs = {"sum": consumer.add(left, right), "equal": consumer.eq(left, right)}
        mapping = {"left": ("left", ports["second"]), "right": ("right", ports["second"])}
        self.assertEqual(joined[0].observe(consumer, outputs, mapping, runtimes),
                         {"sum": 44, "equal": False})
        runtimes["right-crossed"].patch({"delta": 38})
        with self.assertRaises(ValueError):
            joined[0].observe(consumer, outputs, mapping, runtimes)

    def test_compiled_output_adapter_rebuilds_all_fragment_imports_without_evaluation(self):
        program, _, _, chains, consumers = three_consumers()
        plan = discover(program, consumers)
        state = {"left": -3, "right": -2}
        runtime = program.instance(state)
        cache_before, trace_before = dict(runtime.cache), copy.deepcopy(runtime.last_trace)
        reference = plan.observe(program.instance(state), "row")["packets"]
        # Stand-in backend output is computed from the independent recurrence,
        # without using Program operators or the adapter's reconstructed imports.
        values = {chains[side][stage - 1]["size"]: reference_sizes(state[side], stage)[-1]
                  for side in ("left", "right") for stage in (48, 64)}
        with patch.object(runtime, "observe", side_effect=AssertionError("unexpected evaluation")), \
             patch.object(runtime, "_evaluate", side_effect=AssertionError("unexpected evaluation")):
            request = TouchRequest.capture(plan, runtime, "row")
            self.assertEqual(request.outputs, {node: node for node in sorted(values)})
            packets = request.finish(runtime, values)
        self.assertEqual(packets, reference)
        self.assertEqual(len(packets), 4)
        self.assertEqual(runtime.cache, cache_before)
        self.assertEqual(runtime.last_trace, trace_before)
        # The second fragment in each branch imports the compiled stage48 size;
        # neither that value nor any other intermediate entered Runtime.cache.
        for packet in packets:
            self.assertTrue(packet.valid(runtime))
            for node, value in packet.inputs:
                if node in values:
                    self.assertEqual(value, values[node])
                    self.assertNotIn(node, runtime.cache)

    def test_compiled_output_adapter_rejects_changed_support_and_allows_unrelated_patch(self):
        program, _, _, _, consumers = three_consumers()
        program.input("unrelated")
        plan = discover(program, consumers)
        state = {"left": -3, "right": -2, "unrelated": 5}
        runtime = program.instance(state)
        request = TouchRequest.capture(plan, runtime, "row")
        reference_runtime = program.instance(state)
        values = reference_runtime.observe(request.outputs)
        expected = plan.observe(reference_runtime, "row")["packets"]
        runtime.patch({"unrelated": 99})
        cache_before = dict(runtime.cache)
        self.assertEqual(request.finish(runtime, values), expected)
        self.assertEqual(runtime.cache, cache_before)
        runtime.patch({"left": -4})
        changed_cache = dict(runtime.cache)
        with self.assertRaises(ValueError):
            request.finish(runtime, values)
        self.assertEqual(runtime.cache, changed_cache)

    def test_compiled_output_adapter_requires_complete_exact_typed_output_schema(self):
        program, inputs, _, _, consumers = three_consumers()
        plan = discover(program, consumers)
        state = {"left": -3, "right": -2}
        runtime = program.instance(state)
        request = TouchRequest.capture(plan, runtime, "row")
        values = program.instance(state).observe(request.outputs)
        omitted = dict(values)
        first = next(iter(omitted))
        omitted.pop(first)
        extra = dict(values)
        extra[inputs["left"]] = state["left"]
        for malformed in (omitted, extra):
            with self.assertRaises(ValueError):
                request.finish(runtime, malformed)
        for wrong_type in (True, 1.25, "100", None):
            malformed = dict(values)
            malformed[first] = wrong_type
            with self.assertRaises(TypeError):
                request.finish(runtime, malformed)
        self.assertEqual(runtime.cache,
                         {inputs["left"]: state["left"], inputs["right"]: state["right"]})

    def test_compiled_output_adapter_distinguishes_boolean_output_from_integer(self):
        program = Program()
        source = program.input("source")
        flag = program.lt(source, program.const(0))
        plan = discover(program, {"a": {"flag": flag}, "b": {"flag": flag}})
        runtime = program.instance({"source": -3})
        request = TouchRequest.capture(plan, runtime, "row")
        packets = request.finish(runtime, {flag: True})
        self.assertIs(packets[0].value(flag), True)
        with self.assertRaises(TypeError):
            request.finish(runtime, {flag: 1})

    def test_compiled_output_adapter_rejects_foreign_runtime_and_unbound_support(self):
        program, _, _, _, consumers = three_consumers()
        plan = discover(program, consumers)
        state = {"left": -3, "right": -2}
        runtime = program.instance(state)
        request = TouchRequest.capture(plan, runtime, "row")
        other, _, _, _, _ = three_consumers()
        foreign = other.instance(state)
        with self.assertRaises(ValueError):
            TouchRequest.capture(plan, foreign, "row")
        with self.assertRaises(ValueError):
            request.finish(foreign, {})
        late = program.input("late")
        shared = program.mul_const(2, late)
        late_plan = discover(program, {"a": {"value": shared}, "b": {"value": shared}})
        with self.assertRaises(ValueError):
            TouchRequest.capture(late_plan, runtime, "row")
        runtime.patch({"late": 5})
        late_request = TouchRequest.capture(late_plan, runtime, "row")
        self.assertEqual(late_request.finish(runtime, {shared: 10})[0].value(shared), 10)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "out/touchpoint-tests/report.json")
    args = parser.parse_args()
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(TouchpointTests)
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
