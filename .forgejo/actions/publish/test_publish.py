import hashlib
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import urllib.error

import publish


class PublisherTests(unittest.TestCase):
    def test_partial_platforms_are_rejected_before_network(self):
        with tempfile.TemporaryDirectory() as folder:
            p=Path(folder)/'aria-x86_64-unknown-linux-musl.tar.gz'
            p.write_bytes(b'candidate')
            p.with_name(p.name+'.sha256').write_text(hashlib.sha256(p.read_bytes()).hexdigest())
            with patch.dict(os.environ,{'RELEASE_REPOSITORY':'aria','RELEASE_TAG':'v0.1.15','RELEASE_DIRECTORY':folder}),patch.object(publish.urllib.request,'build_opener') as opener:
                with self.assertRaisesRegex(RuntimeError,'platform'):publish.main()
                opener.assert_not_called()

    def test_fallback_failure_keeps_forgejo_draft(self):
        calls=[]
        with tempfile.TemporaryDirectory() as folder:
            p=Path(folder)/'fanctl-x86_64-unknown-linux-musl.tar.gz'
            p.write_bytes(b'candidate');p.with_name(p.name+'.sha256').write_text(publish.digest(p))
            assets=[{'name':f.name,'digest':'sha256:'+publish.digest(f)} for f in Path(folder).iterdir()]
            class Opener:
                def open(self,request,timeout):
                    calls.append((request.full_url,request.get_method()))
                    if request.full_url.startswith(publish.GITHUB):raise urllib.error.HTTPError(request.full_url,503,'unavailable',{},None)
                    return io.BytesIO(json.dumps([{'id':1,'tag_name':'v0.1.2','assets':assets,'draft':True}]).encode())
            env={'RELEASE_REPOSITORY':'fiato','RELEASE_TAG':'v0.1.2','RELEASE_DIRECTORY':folder,'RELEASE_WRITE_TOKEN':'test','GITHUB_RELEASE_SYNC_TOKEN':'test'}
            with patch.dict(os.environ,env),patch.object(publish.urllib.request,'build_opener',return_value=Opener()),patch.object(publish.subprocess,'run') as run:
                with self.assertRaises(urllib.error.HTTPError):publish.main()
                run.assert_not_called()
            self.assertTrue(all(method=='GET' for _,method in calls))


if __name__=='__main__':unittest.main()
