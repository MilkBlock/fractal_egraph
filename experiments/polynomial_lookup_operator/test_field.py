import unittest
from unittest.mock import patch
import numpy as np
from field import *
from prepare import build_bank


class FieldTests(unittest.TestCase):
    def test_membership_and_bucket_payload(self):
        rng=np.random.default_rng(14)
        for n in (1,7,31,127):
            roots=rng.choice(PRIME,size=n,replace=False);p=PolynomialIndex(roots)
            np.testing.assert_array_equal(evaluate(p.p,roots),0)
            np.testing.assert_array_equal(evaluate(p.f,roots),[p.handles[int(r)] for r in roots])

    def test_insert_remove_are_coefficient_updates(self):
        p=PolynomialIndex([0,1,17,65520]);p.insert(4321,41)
        np.testing.assert_array_equal(evaluate(p.f,list(p.handles)),list(p.handles.values()))
        p.remove(1);p.remove(0)
        self.assertEqual(p.roots,sorted(p.handles))
        np.testing.assert_array_equal(evaluate(p.p,list(p.handles)),0)
        np.testing.assert_array_equal(evaluate(p.f,list(p.handles)),list(p.handles.values()))
        self.assertNotEqual(int(evaluate(p.p,1)),0)

    def test_half_digit_matrix_exact(self):
        rng=np.random.default_rng(29)
        for k in (7,128,512):
            a=rng.integers(0,PRIME,size=(13,k),dtype=np.int64)
            b=rng.integers(0,PRIME,size=(k,17),dtype=np.int64)
            np.testing.assert_array_equal(gemm_mod(a,b),(a@b)%PRIME)

    def test_collision_never_selects_one_original_key(self):
        p=PolynomialIndex([5,5,9]);self.assertEqual(len(p.handles),2)
        self.assertEqual(int(evaluate(p.f,5)),p.handles[5])
        with self.assertRaises(ValueError):p.insert(5,99)

    def test_raw_id_hash_does_not_use_table_membership(self):
        # The same key gets the same coordinates whether it is observed alone,
        # among unrelated keys, or absent from the eventual target table.
        keys=np.array([[3,2**63+77,91],[3,444,91],[3,2**63+77,92]],dtype=np.uint64)
        np.testing.assert_array_equal(hash_keys(keys)[0],hash_keys(keys[:1])[0])
        self.assertFalse(np.array_equal(hash_keys(keys)[0],hash_keys(keys)[2]))

    def test_deliberate_collision_keeps_both_row_payloads(self):
        rows=np.array([[11,22],[33,44]],dtype=np.uint64)
        probes=np.array([[3,11,22],[3,33,44]],dtype=np.uint64)
        def collide(keys):return np.tile([5,7],(len(keys),1))
        with patch('prepare.hash_keys',collide):
            _,_,_,_,payload,original,_=build_bank(rows,probes,127)
        self.assertEqual(payload,[[[0,1]]])
        np.testing.assert_array_equal(original,probes)

    def test_remove_last_root_and_reinsert(self):
        p=PolynomialIndex([27]);p.remove(27);p.insert(99,15)
        self.assertEqual(int(evaluate(p.p,99)),0)
        self.assertEqual(int(evaluate(p.f,99)),15)


if __name__=='__main__':unittest.main()
