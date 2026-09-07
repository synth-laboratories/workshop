import tempfile
import unittest
from pathlib import Path
from environment_qa.image_visibility import copied_file_paths,visibility_command

class VisibilityTests(unittest.TestCase):
    def test_directory_copy_maps_original_files_not_injected_sources(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);e=root/'environment';(e/'resources').mkdir(parents=True)
            (e/'resources/answer').write_text('fixture')
            (e/'Dockerfile').write_text('WORKDIR /app\nCOPY resources /app/resources\nCOPY ../../outside /tmp/secret\n')
            paths=copied_file_paths(root)
            self.assertEqual(paths,['/app/resources/answer'])
            self.assertNotIn('qa-review-sources',visibility_command(paths))
            self.assertIn('READABLE /app/resources/answer',visibility_command(paths))
