import json
from pathlib import Path
import unittest
from pronunciation import spoken_text


class Pronunciation(unittest.TestCase):
    def test_identifiers_are_spelled_not_renamed(self):
        self.assertEqual(spoken_text('先读b0，再取a1和b1。'),
                         '先读字母 B，零，再取字母 A，一和字母 B，一。')
        self.assertEqual(spoken_text('A和B共享t。'), '字母 A和字母 B共享字母 T。')

    def test_only_complete_tokens_change(self):
        self.assertEqual(spoken_text('data a10 b01 stable alpha_a0 a0_extra'),
                         'data a10 b01 stable alpha_a0 a0_extra')

    def test_display_script_retains_original_identifiers(self):
        script=json.loads(Path(__file__).with_name('narration.json').read_text())
        beats={key:text for key,_,text in script['beats']}
        self.assertIn('b0',beats['first_read']);self.assertIn('a0',beats['first_read'])
        self.assertNotIn('字母',beats['first_read'])
        for text in beats.values():
            self.assertFalse(any(c in text for c in '甲乙梯'),text)
            self.assertFalse(any(c in spoken_text(text) for c in '甲乙梯'),text)


if __name__=='__main__':unittest.main()
