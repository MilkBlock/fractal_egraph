"""HTTP/NDJSON failure regression; does not need a native binary or renderer."""
import json
from pathlib import Path
import tempfile
import threading
import unittest
from unittest.mock import patch
from http.server import ThreadingHTTPServer
from urllib.request import Request, urlopen
from urllib.error import HTTPError
from server import Handler


class StreamErrors(unittest.TestCase):
    def test_missing_binary_and_spawn_race(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            srv = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
            srv.binary = root / 'runtime'
            srv.allowed_origins = set()
            thread = threading.Thread(target=srv.serve_forever, daemon=True)
            thread.start()
            url = f'http://127.0.0.1:{srv.server_port}/api/trace'
            def request():
                return urlopen(Request(url, data=b'{"source":"(datatype E (A))"}',
                                       headers={'Content-Type': 'application/json'}))
            try:
                with self.assertRaises(HTTPError) as failure:
                    request()
                self.assertEqual(failure.exception.code, 503)
                self.assertIn('runtime', json.load(failure.exception)['error'])
                failure.exception.close()
                srv.binary.write_text('placeholder')
                srv.binary.chmod(0o755)
                # Binary can disappear after preflight, before Popen.
                with patch('server.ROOT', root), patch('server.subprocess.Popen', side_effect=FileNotFoundError('spawn race')):
                    with request() as response:
                        self.assertEqual(response.status, 200)
                        self.assertEqual(response.headers['Content-Type'], 'application/x-ndjson')
                        rows = [json.loads(line) for line in response.read().splitlines()]
                        self.assertEqual(rows, [{'kind': 'error', 'error': 'spawn race'}])
            finally:
                srv.shutdown()
                srv.server_close()
                thread.join()


if __name__ == '__main__':
    unittest.main()
