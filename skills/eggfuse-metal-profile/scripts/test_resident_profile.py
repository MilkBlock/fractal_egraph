import copy,json,tempfile,unittest
from pathlib import Path
from unittest.mock import patch
from resident_full import verify,summarize
from resident_window import instrument
from artifacts import mirror_summaries,archive_capture,capture_filesystem
import resident


def state():
    return dict(program_sha256='source',restored_runs=[0],instances=[dict(completed=True,meta=[0,0,0,0,0],history=[[1]],rows=[[7]])],private_schedule_execution=True,private_schedule_regions=2,private_schedule_runs=4,private_schedule_rows=3,private_schedule_forwarded_reads=5,private_schedule_binding_outputs=6,dispatches=8,inference_seconds=10)

class ResidentProfilingTests(unittest.TestCase):
    def test_summary_mirror_excludes_graph_and_binary_artifacts(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp)/'run';root.mkdir();dest=Path(tmp)/'mirror'
            (root/'result.json').write_text('large graph');(root/'summary.json').write_text('{"verified":true}')
            mirror_summaries(root,dest)
            self.assertFalse((dest/'result.json').exists())
            self.assertEqual((dest/'summary.json').read_text(),'{"verified":true}')
            self.assertEqual(len(json.loads((dest/'mirror.json').read_text())['files']),1)

    def test_capture_archive_is_verified_non_destructive_and_never_overwrites(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source=root/'test.gputrace';source.mkdir();(source/'data').write_bytes(b'fixture')
            (source/'alias').symlink_to('data')
            dest=root/'archive.gputrace.tar';result=archive_capture(source,dest)
            self.assertTrue(result['byte_verified']);self.assertFalse(result['apple_semantic_validation'])
            import tarfile
            with tarfile.open(dest) as archive:
                self.assertEqual((source/'data').read_bytes(),archive.extractfile('test.gputrace/data').read())
                self.assertEqual(archive.getmember('test.gputrace/alias').linkname,'data')
            with self.assertRaises(ValueError):archive_capture(source,dest)
            import plistlib
            with patch('artifacts.subprocess.check_output',return_value=plistlib.dumps(dict(FilesystemType='ExFAT'))):
                with self.assertRaisesRegex(ValueError,'APFS'):capture_filesystem(root)

    def test_reject_changed_private_execution_or_state(self):
        a=state();verify(a,copy.deepcopy(a))
        for k,v in [('private_schedule_execution',False),('private_schedule_runs',0),('dispatches',7),('program_sha256','other')]:
            b=copy.deepcopy(a);b[k]=v
            with self.assertRaises(ValueError):verify(a,b)
        b=copy.deepcopy(a);b['instances'][0]['rows']=[[8]]
        with self.assertRaises(ValueError):verify(a,b)

    def test_accounting_does_not_add_cpu_wait_or_claim_barrier_time(self):
        a=state();b=state();a['inference_seconds']=12;a['dispatches']=b['dispatches']=1
        a['full_encoder_profile']=dict(pending_commands=0,phases={'resident_merge_row_index_parallel':dict(seconds=2,issued_encoders=3),'private_sm_mma':dict(seconds=1,issued_encoders=2),'resident_packet_publish':dict(seconds=1,issued_encoders=2)},commands=[dict(gpu_seconds=5,encoder_interval_union_seconds=4)],cpu_wait_calls=2,cpu_wait_wall_seconds=6,cpu_command_encoding_wall_seconds=1,counter_resolution_wall_seconds=.1)
        r=summarize(a,b)
        self.assertEqual(r['all_command_gpu_seconds'],5)
        self.assertEqual(r['unattributed_command_seconds'],1)
        self.assertEqual(r['groups']['merge_index_refresh']['seconds'],2)
        self.assertEqual(r['groups']['private_schedule']['seconds'],1)
        self.assertEqual(r['groups']['rhs_publish']['seconds'],1)
        a['full_encoder_profile']['commands'][0]['encoder_interval_union_seconds']=50
        with self.assertRaisesRegex(ValueError,'interval union|timestamp'):summarize(a,b)

    def test_window_adapter_rejects_unrecognized_controller(self):
        with self.assertRaisesRegex(ValueError,'anchor'):instrument('unrecognized worker',4,1,'counters')

    def test_capture_default_reference_runs_exactly_once(self):
        # Exercises the former undefined `runs` branch without requiring Metal.
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);cp=root/'checkpoint';cp.mkdir();program=root/'program.json';program.write_text('{}')
            (cp/'manifest.json').write_text(json.dumps(dict(schema='eggfuse-resident-checkpoint/v1',instances=1,completed_runs=[3],program_sha256='p')))
            calls=[]
            def replay(program,build,out,checkpoint,stop,**kw):
                calls.append(stop);out.mkdir(parents=True)
                result=dict(program_sha256='p',worker_sha256='w',restored_runs=[3],instances=[dict(history=[[0]]*4)])
                (out/'result.json').write_text(json.dumps(result))
                if kw.get('capture'):
                    kw['capture'].mkdir();(kw['capture']/'data').write_text('capture')
                return dict(worker_sha256='w')
            # Imported inside record; patch the real public adapter seam.
            import sys
            repo=Path(__file__).resolve().parents[3];sys.path.insert(0,str(repo))
            with patch('eggfuse.engine.replay.replay',side_effect=replay),patch('resident.subprocess.check_output',return_value='Xcode test'):
                r=resident.record(repo,program,cp,root/'build',root/'capture')
            self.assertEqual(calls,[4,4]);self.assertTrue(r['full_state_and_history_equal'])

if __name__=='__main__':unittest.main()
