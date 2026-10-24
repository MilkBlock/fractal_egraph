import unittest
from append import merge_edits


class Append(unittest.TestCase):
    def edit(self,key,duration):
        return dict(renderer='Manim',width=1920,height=1080,fps=30,duration=duration,
                    chapters=[dict(title=key,start=0,end=duration)],captions=[dict(key=key,start=.2,end=duration-.2,text=key)])

    def test_frame_exact_offset_without_mutating_inputs(self):
        a=self.edit('a',100/30);b=self.edit('b',20/30)
        result=merge_edits(a,b,100)
        self.assertEqual(result['duration'],4)
        self.assertAlmostEqual(result['captions'][1]['start'],100/30+.2)
        self.assertEqual(result['captions'][0],a['captions'][0])
        self.assertEqual(len(a['captions']),1);self.assertEqual(b['chapters'][0]['start'],0)

    def test_rejects_colliding_ids_or_wrong_timeline(self):
        a=self.edit('a',1);b=self.edit('a',2)
        with self.assertRaises(ValueError):merge_edits(a,b,30)
        with self.assertRaises(ValueError):merge_edits(a,self.edit('b',2),31)
        b=self.edit('b',2);b['fps']=15
        with self.assertRaises(ValueError):merge_edits(a,b,30)


if __name__=='__main__':unittest.main()
