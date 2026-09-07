import unittest
from environment_qa.source_inventory import candidates,inventory

class InventoryTests(unittest.TestCase):
    def test_declared_root_build_hook_is_followed(self):
        def read(url):
            if url.endswith('pyproject.toml'):return '[tool.poetry]\nbuild = "build.py"\n'
            if url.endswith('build.py'):return 'import legacy_build_api\n'
            return ''
        result=inventory({'solution/solve.sh':'git clone --branch 1.0 https://github.com/org/pkg.git /app/pkg'},read)
        self.assertTrue(any(r['url'].endswith('/build.py') for r in result['records']))
    def test_relative_and_function_local_imports(self):
        from environment_qa.source_inventory import imported_sources
        urls=imported_sources({'url':'https://raw.githubusercontent.com/org/pkg/v1/pkg/sub/model.py','kind':'upstream_source','status':'retrieved','body':'def f():\n    from ..adapter import run\n    from . import helper\n    import pkg.other\n    import unrelated.secret\n'})
        self.assertIn('https://raw.githubusercontent.com/org/pkg/v1/pkg/adapter.py',urls)
        self.assertIn('https://raw.githubusercontent.com/org/pkg/v1/pkg/sub/helper.py',urls)
        self.assertIn('https://raw.githubusercontent.com/org/pkg/v1/pkg/other.py',urls)
        self.assertFalse(any('unrelated' in url for url in urls))
    def test_only_named_source_refs(self):
        files={'solution/solve.sh':'V=1.2\ngit clone --branch ${V} https://github.com/org/lib.git /tmp/lib\nhttps://github.com/org/lib/pull/1\nhttp://127.0.0.1/secret'}
        items=candidates(files)
        self.assertEqual(len(items),4)
        self.assertTrue(all(u.startswith('https://raw.githubusercontent.com/org/lib/1.2/') for u,_ in items))
    def test_transitive_metadata_and_provenance(self):
        def read(url):
            if '/parent/' in url: return 'Package: parent\nImports: child (>= 1)\n'
            return 'Package: child\nSystemRequirements: cmake\n'
        result=inventory({'solution/solve.sh':"install.packages('parent')"},read)
        self.assertEqual(len(result['records']),2)
        self.assertEqual(result['findings'],[])
        self.assertTrue(all(r['sha256'] for r in result['records']))
    def test_unavailable_not_a_finding(self):
        def fail(url): raise OSError('network unavailable')
        result=inventory({'x':"install.packages('parent')"},fail)
        self.assertEqual(result['findings'],[])
        self.assertEqual(result['records'][0]['status'],'unavailable')

    def test_task_named_module_and_local_imports_are_followed(self):
        text='git clone --branch 1.0 https://github.com/org/pkg.git /app/pkg\nsed -i x /app/pkg/pkg/consumer.py'
        def read(url):
            return 'from pkg.adapter import Adapter\n' if url.endswith('consumer.py') else 'value=1\n'
        result=inventory({'solution/solve.sh':text},read)
        self.assertTrue(any(r['url'].endswith('/pkg/adapter.py') for r in result['records']))
