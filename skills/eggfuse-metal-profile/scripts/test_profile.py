import io,json
from pathlib import Path
import tempfile
import unittest
import xml.etree.ElementTree as ET
from counters import Coverage, summarize as summarize_counters
from report import intervals_union, tables, summarize, trace_problems, cpu_samples
from capture_report import duration_ns, json_records
from profile import replace_once


class ProfileTests(unittest.TestCase):
    def test_overlap_accounting(self):
        spans = [(10, 30), (20, 40), (50, 60), (10, 30)]
        self.assertEqual(intervals_union(spans), [[10, 40], [50, 60]])
        c = Coverage(spans)
        self.assertEqual(c.overlap(25, 55), 20)
        self.assertEqual(c.overlap(-10, 100), 40)
        self.assertEqual(c.overlap(40, 50), 0)

    def test_export_references_and_nested_values(self):
        xml = '''<trace-query-result><node><schema name="test">
          <col><mnemonic>process</mnemonic></col><col><mnemonic>duration</mnemonic></col>
          </schema><row><process id="1" fmt="worker (42)"><pid id="2">42</pid></process>
          <duration id="3" fmt="2 us">2000</duration></row>
          <row><process ref="1"/><duration ref="3"/></row></node></trace-query-result>'''
        with tempfile.TemporaryDirectory() as d:
            p = Path(d)/'export.xml'; p.write_text(xml)
            rows = tables(p)[0]['rows']
            self.assertEqual(rows[0], rows[1])
            self.assertEqual(rows[1]['duration']['raw'], '2000')

    def test_grouped_encoders_are_not_misattributed_to_first_kernel(self):
        columns = ['start','duration','process','state','event-depth','event-label','encoder-id','cmdbuffer-id']
        rows = [
            [0,100,'worker (42)','Active',0,'fn=A threads=10 indirect=false & fn=B threads=1 indirect=false',1,1],
            [10,30,'worker (42)','Active',1,'nested',1,1],
            [200,100,'worker (42)','Active',0,'fn=A threads=10 indirect=false',2,2],
            [0,300,'other (43)','Active',0,'other',3,3]]
        root = ET.Element('trace-query-result'); node = ET.SubElement(root,'node')
        schema = ET.SubElement(node,'schema', name='metal-gpu-intervals')
        for name in columns:ET.SubElement(ET.SubElement(schema,'col'),'mnemonic').text=name
        for values in rows:
            row=ET.SubElement(node,'row')
            for value in values:ET.SubElement(row,'value',fmt=str(value)).text=str(value)
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);ET.ElementTree(root).write(p/'metal-gpu-intervals.xml')
            (p/'availability.json').write_text(json.dumps({'rows':{},'trace_issues':[]}))
            result=summarize(p,42)
            self.assertEqual(result['gpu_active_time_fraction'],2/3)
            self.assertEqual(result['nested_gpu_segments_excluded'],1)
            self.assertEqual({r['kernel'] for r in result['encoder_groups']},{'A+B','A'})
            self.assertEqual(result['shader_hotspots'],[])

    def test_error_and_metadata_distinction(self):
        header={'type':2,'subtype':0,'message':'Data stream: Time Mapping'}
        error={'type':1,'subtype':0,'message':'Transferred trace file is malformed'}
        self.assertEqual(trace_problems([header]),[])
        self.assertEqual(trace_problems([header,error]),[error])

    def test_capture_values_do_not_confuse_cost_with_duration(self):
        self.assertEqual(duration_ns('910.8293 ms'),910829300)
        self.assertIsNone(duration_ns('26.31%'))
        self.assertEqual(duration_ns('2.4 µs'),2400)
        self.assertEqual(json_records('gpudebug> {"ok":true}\nprompt> [1,2]'),[{'ok':True},[1,2]])

    def test_instrumentation_fails_closed_on_source_drift(self):
        self.assertEqual(replace_once('abc','b','x'),'axc')
        with self.assertRaises(ValueError):replace_once('abbc','b','x')
        with self.assertRaises(ValueError):replace_once('abc','z','x')

    def test_wait_stack_is_not_misclassified_by_running_label(self):
        xml='''<trace-query-result><node><schema name="time-profile">
        <col><mnemonic>time</mnemonic></col><col><mnemonic>thread</mnemonic></col>
        <col><mnemonic>process</mnemonic></col><col><mnemonic>thread-state</mnemonic></col>
        <col><mnemonic>stack</mnemonic></col></schema>
        <row><time>25</time><thread id="1" fmt="Main Thread"/><process id="2" fmt="worker (42)"/>
        <state id="3">Running</state><stack id="4"><frame name="__psynch_cvwait"/>
        <frame name="-[MTLCommandBuffer waitUntilCompleted]"/></stack></row>
        <row><time>500</time><thread ref="1"/><process ref="2"/><state ref="3"/><stack ref="4"/></row>
        </node></trace-query-result>'''
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'cpu.xml';p.write_text(xml)
            result=cpu_samples(p,42,0,100)
            self.assertEqual(result['main_thread_samples_in_gpu_span'],1)
            self.assertEqual(result['wait_until_completed_stacks'],1)
            self.assertEqual(result['reported_thread_states'],{'Running':1})

    def test_counter_average_is_time_weighted_and_clipped(self):
        gpu='''<trace-query-result><node><schema name="metal-gpu-intervals">
        <col><mnemonic>start</mnemonic></col><col><mnemonic>duration</mnemonic></col>
        <col><mnemonic>process</mnemonic></col><col><mnemonic>state</mnemonic></col>
        <col><mnemonic>event-depth</mnemonic></col></schema><row>
        <v>50</v><v>100</v><v fmt="worker (42)"/><v>Active</v><v>0</v></row></node></trace-query-result>'''
        columns=['start','duration','name','value','is-percentage','label']
        root=ET.Element('trace-query-result');node=ET.SubElement(root,'node')
        schema=ET.SubElement(node,'schema',name='metal-gpu-counter-intervals')
        for c in columns:ET.SubElement(ET.SubElement(schema,'col'),'mnemonic').text=c
        # Only 50ns of each 100ns sample overlaps the target; the last sample
        # must not contribute despite its much larger value.
        for lo,value in [(0,10),(100,30),(200,1000)]:
            row=ET.SubElement(node,'row')
            for val in [lo,100,'Kernel Occupancy',value,1,'occupancy']:
                ET.SubElement(row,'v',fmt=str(val)).text=str(val)
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);(p/'gpu.xml').write_text(gpu);ET.ElementTree(root).write(p/'counter.xml')
            r=summarize_counters(p/'counter.xml',p/'gpu.xml',42)
            value=r['groups']['all_target_active']['counters']['Kernel Occupancy']
            self.assertEqual(value['mean'],20)
            self.assertEqual(value['peak'],30)
            self.assertEqual(value['samples'],2)
            # xctrace stdout is a stream, not a mandatory multi-GB XML file.
            streamed=summarize_counters(io.BytesIO((p/'counter.xml').read_bytes()),p/'gpu.xml',42)
            self.assertEqual(streamed,r)
            self.assertFalse(r['attribution']['per_shader_attribution_available'])
            self.assertEqual(r['attribution']['target_shader_sample_coverage_fraction'],0)

    def test_shader_coverage_and_other_process_overlap_are_not_summed(self):
        from counters import attribution_context
        def export(path,cols,rows):
            root=ET.Element('trace-query-result');node=ET.SubElement(root,'node');schema=ET.SubElement(node,'schema',name='fixture')
            for col in cols:ET.SubElement(ET.SubElement(schema,'col'),'mnemonic').text=col
            for values in rows:
                row=ET.SubElement(node,'row')
                for value in values:ET.SubElement(row,'v',fmt=str(value)).text=str(value)
            ET.ElementTree(root).write(path)
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)
            export(p/'gpu.xml',['start','duration','process','state','event-depth'],[
                [0,100,'worker (42)','Active',0],[10,20,'other (43)','Active',0],[20,20,'third (44)','Active',0]])
            export(p/'metal-shader-profiler-intervals.xml',['start','duration','process'],[[50,10,'worker (42)'],[55,20,'worker (42)'],[0,100,'other (43)']])
            r=attribution_context(p/'gpu.xml',42)
            self.assertEqual(r['other_process_overlap_fraction'],.3)
            self.assertEqual(r['target_shader_sample_coverage_fraction'],.25)


if __name__ == '__main__':unittest.main()
