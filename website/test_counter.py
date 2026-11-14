"""Run from repo root: python -m unittest website.test_counter"""
import json
import tempfile
import unittest
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from website import wsgi

class CounterTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.old_path = wsgi.DATA_PATH
        wsgi.DATA_PATH = Path(self.temp.name) / 'visits.sqlite3'
    def tearDown(self):
        wsgi.DATA_PATH = self.old_path
        self.temp.cleanup()
    def request(self, path, method='GET', cookie='', origin='https://zapture.pythonanywhere.com'):
        result = {}
        def start(status, headers): result.update(status=status, headers=dict(headers))
        result['body'] = b''.join(wsgi.application({'PATH_INFO':path,'REQUEST_METHOD':method,'HTTP_COOKIE':cookie,'HTTP_ORIGIN':origin,'HTTP_X_ZAPTURE_VISIT':'1'},start))
        return result
    def test_daily_cookie_deduplicates(self):
        first=self.request('/api/visit','POST')
        self.assertEqual(json.loads(first['body'])['visits'],1)
        cookie=first['headers']['Set-Cookie'].split(';')[0]
        second=self.request('/api/visit','POST',cookie)
        self.assertEqual(json.loads(second['body'])['visits'],1)
        third=self.request('/api/visit','POST','zapture_visit=2000-01-01')
        self.assertEqual(json.loads(third['body'])['visits'],2)
    def test_badges_stats_and_head_never_increment(self):
        for path in ['/visitor-badge.svg','/api/stats']:
            self.assertEqual(self.request(path)['status'],'200 OK')
            self.assertEqual(self.request(path,'HEAD')['body'],b'')
        self.assertEqual(wsgi.visit_count(),0)
    def test_reject_cross_origin_and_wrong_method(self):
        self.assertEqual(self.request('/api/visit','POST',origin='https://example.org')['status'],'403 Forbidden')
        self.assertEqual(self.request('/api/visit')['status'],'405 Method Not Allowed')
        self.assertEqual(wsgi.visit_count(),0)
    def test_atomic_concurrent_updates(self):
        wsgi.visit_count()
        with ThreadPoolExecutor(max_workers=6) as pool:
            list(pool.map(lambda _:wsgi.visit_count(True),range(20)))
        self.assertEqual(wsgi.visit_count(),20)
    def test_private_files_not_served(self):
        for path in ['/wsgi.py','/../zapture-data/visits.sqlite3','/test_counter.py']:
            self.assertTrue(self.request(path)['status'].startswith('404'))

if __name__=='__main__':unittest.main()
