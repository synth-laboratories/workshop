import unittest
from environment_qa.review_context import ancestral

class ReviewContextTests(unittest.TestCase):
    def test_peer_completion_cannot_change_domain_review_evidence(self):
        run={'gates':[{'id':'source','depends_on':[]},{'id':'attribution','depends_on':['source']},
                      {'id':'critic','depends_on':['attribution']},{'id':'domain','depends_on':['attribution']}],
             'evidence':[{'gate':'source'},{'gate':'attribution'},{'gate':'critic'},{'gate':'seeded-runtime'}],
             'findings':[{'id':'f','gate_id':'source','disposition':'unresolved','assessments':[
                 {'gate_id':'attribution','status':'dismissed'},{'gate_id':'critic','status':'confirmed'}]}]}
        result=ancestral(run,{'id':'domain'})
        self.assertEqual([e['gate'] for e in result['evidence']],['source','attribution','seeded-runtime'])
        self.assertEqual(result['findings'][0]['disposition'],'dismissed')
        self.assertEqual(len(result['findings'][0]['assessments']),1)
        self.assertEqual(run['findings'][0]['disposition'],'unresolved')

    def test_seeded_replay_inputs_remain_visible(self):
        run={'gates':[{'id':'critic','depends_on':[]}],'evidence':[{'gate':'source'},{'gate':'attribution'}],
             'findings':[{'id':'f','gate_id':'source','disposition':'proposed'}]}
        self.assertEqual(ancestral(run,{'id':'critic'}),run)
