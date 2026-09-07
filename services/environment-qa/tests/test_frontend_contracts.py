import unittest
from environment_qa.frontend_contracts import contracts, merge_required

class FrontendContractsTests(unittest.TestCase):
    def test_mandatory_check_does_not_push_out_a_distinct_protocol(self):
        required=[{'package':'poetry build frontend','source_path':'solution/solve.sh','evidence':'python -m poetry build','probe':'preserve exact hook'}]
        proposed=[{'package':'Poetry backend','source_path':'external/pyproject','evidence':'backend','consumer_path':'solution/solve.sh','consumer_evidence':'python -m poetry build','probe':'tiny package','candidate_ids':['0']},
                  *[{'source_path':'external/api','evidence':str(i),'probe':'stateful protocol','candidate_ids':[str(i)]} for i in range(1,6)]]
        merged=merge_required(required,proposed)
        self.assertEqual(len(merged),6)
        self.assertEqual(merged[-1]['candidate_ids'],['5'])
        self.assertIn('preserve exact hook',merged[0]['probe'])
        self.assertEqual(proposed[0]['probe'],'tiny package')

    def test_incidental_downstream_build_quote_is_not_a_frontend_check(self):
        required=[{'package':'poetry build frontend','source_path':'solution/solve.sh','evidence':'python -m poetry build','probe':'actual frontend'}]
        proposed=[{'package':'git source endpoint','consumer_path':'solution/solve.sh','consumer_evidence':'git clone source\npython -m poetry build','probe':'git ls-remote'}]
        result=merge_required(required,proposed)
        self.assertEqual(len(result),2)
        self.assertTrue(result[0]['required_frontend'])
        self.assertNotIn('required_frontend',result[1])
    def test_actual_build_invocation_requires_smoke(self):
        text='python -m pip install "poetry~=1.8"\npython -m poetry -C /tmp/project build -v\n'
        result=contracts({'solution/solve.sh':text})
        self.assertEqual(len(result),1)
        self.assertIn(result[0]['evidence'],text)
        self.assertIn(result[0]['relevance_evidence'],text)
        self.assertIn('minimal empty package',result[0]['probe'])
        self.assertIn('hook registration',result[0]['probe'])
        self.assertIn('INSIDE the invoked hook',result[0]['probe'])
        self.assertIn('versions inside the build environment',result[0]['probe'])
    def test_comments_and_version_query_are_not_build(self):
        self.assertEqual(contracts({'solution/solve.sh':'# python -m poetry build\npython -m poetry --version'}),[])
