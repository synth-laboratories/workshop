import unittest
import contextlib
import io
import tarfile
import tempfile
from pathlib import Path
from environment_qa.archive_probe import command


class ArchiveProbeTests(unittest.TestCase):
    def test_reads_members_without_executing_or_extracting_them(self):
        with tempfile.TemporaryDirectory() as temp:
            unpack=Path(temp)/'pip-unpack-fixture';unpack.mkdir()
            archive=unpack/'small-package-1.0.tar.gz'
            payload=b'raise RuntimeError("THIS MUST NOT EXECUTE")\n'
            with tarfile.open(archive,'w:gz') as stream:
                member=tarfile.TarInfo('small-package/module.py');member.size=len(payload)
                stream.addfile(member,io.BytesIO(payload))
                link=tarfile.TarInfo('small-package/link.py');link.type=tarfile.SYMTYPE;link.linkname='/etc/passwd'
                stream.addfile(link)
            text=command('small-package').split('\n',1)[1].rsplit('\n',1)[0]
            text=text.replace("Path('/tmp')",'Path('+repr(temp)+')')
            output=io.StringIO()
            with contextlib.redirect_stdout(output):exec(compile(text,'archive-reader-test','exec'),{})
            self.assertIn('THIS MUST NOT EXECUTE',output.getvalue())
            self.assertIn('sha256',output.getvalue())
            self.assertNotIn('link.py',output.getvalue())
            self.assertFalse((Path(temp)/'small-package').exists())

    def test_command_is_bounded_read_only_and_valid_python(self):
        text=command('small-package')
        source=text.split('\n',1)[1].rsplit('\n',1)[0]
        compile(source,'archive-probe','exec')
        self.assertIn('SOURCE-ONLY',source)
        self.assertIn('member.isfile()',source)
        self.assertIn('budget = 32000',source)
        self.assertNotIn('.extractall(',source)
        self.assertNotIn('subprocess',source)
    def test_rejects_shell_and_path_injection(self):
        for name in ['pkg; echo bad','../pkg','$(id)','pkg\nnext']:
            with self.assertRaises(ValueError):command(name)
