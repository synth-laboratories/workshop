import unittest
from unittest.mock import patch
from environment_qa.clause_matching import match_clauses

class ClauseMatchingTests(unittest.TestCase):
    def test_independent_scoring_review_retains_disagreement(self):
        initial={'references':{'g':{'0':{'identified':False,'prediction_ids':[],'evidence_level':'none','reason':'Initial rejection'}}},'limitations':[]}
        final={'references':{'g':{'0':{'identified':True,'prediction_ids':['p'],'evidence_level':'conditional_source','reason':'Same concrete property, no runtime proof required'}}},'limitations':[]}
        with patch('environment_qa.clause_matching.request_json',side_effect=[initial,final]) as request:
            result=match_clauses(None,'run','token',[{'id':'p'}],[{'id':'g','clauses':['missing prerequisite']}])
        self.assertEqual(request.call_count,2)
        self.assertEqual(result['matches'][0]['coverage'],'full')
        self.assertEqual(result['matching_review']['disagreements'],[{'gold_id':'g','clause':'0'}])
        self.assertFalse(result['matching_review']['human_confirmed'])

    def test_partial_requires_a_complete_identified_clause(self):
        decisions={'references':{'g':{'0':{'identified':False,'prediction_ids':[],'evidence_level':'none','reason':'Generic dependency risk only'},
                                     '1':{'identified':True,'prediction_ids':['p'],'evidence_level':'conditional_source','reason':'Specific independent race'}}},'limitations':[]}
        with patch('environment_qa.clause_matching.request_json',return_value=decisions):
            result=match_clauses(None,'run','token',[{'id':'p'}],[{'id':'g','clauses':['package failure','race']}])
        self.assertEqual(result['matches'][0]['coverage'],'partial')
        decisions['references']['g']['1']['identified']=False
        with patch('environment_qa.clause_matching.request_json',return_value=decisions):
            self.assertEqual(match_clauses(None,'run','token',[{'id':'p'}],[{'id':'g','clauses':['package failure','race']}])['matches'],[])

    def test_hedged_disjunction_rule_is_declared_for_every_clause(self):
        """A mechanism named only as one of several alternatives is not identified."""
        from unittest.mock import patch as _patch
        captured={}
        decisions={'references':{'g':{'0':{'identified':False,'prediction_ids':[],'evidence_level':'none','reason':'Disjunctive hedge'}}},'limitations':[]}
        def record(store,run_id,gate,messages,**kwargs):
            captured.setdefault('system',messages[0]['content']); return decisions
        with _patch('environment_qa.clause_matching.request_json',side_effect=record):
            match_clauses(None,'run','token',[{'id':'p'}],[{'id':'g','clauses':['swap masks the shortfall']}])
        instruction=captured['system']
        self.assertIn('hedged disjunction',instruction)
        self.assertIn('swap or overcommit',instruction)
        self.assertIn('an unresolved choice between two is not',instruction)
        # The rule must not be scoped to resource mechanisms alone.
        self.assertIn('This applies to every clause',instruction)
