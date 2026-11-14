import tempfile
import unittest
from pathlib import Path
from website.wsgi import create_app

class DashboardTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory()
        self.app=create_app(Path(self.tmp.name), password='test-only-password')
        self.client=self.app.test_client()
    def tearDown(self):self.tmp.cleanup()
    def test_capture_and_protected_dashboard(self):
        page=self.client.get('/',headers={'User-Agent':'Mozilla/5.0 Chrome/130.0.0.0 Safari/537.36'})
        self.assertIn(b'<script src="/visits/track.js"',page.data)
        self.assertEqual(page.headers['Cache-Control'],'private, no-store')
        self.assertEqual(self.client.get('/visits/api/stats').status_code,401)
        self.assertEqual(self.client.post('/visits/login',data={'password':'wrong'}).status_code,401)
        self.assertEqual(self.client.post('/visits/login',data={'password':'test-only-password','me':'1'}).status_code,303)
        stats=self.client.get('/visits/api/stats').get_json()
        self.assertGreaterEqual(stats['kpi']['visits'],1)
        self.client.get('/')
        self.assertEqual(self.client.get('/visits/api/stats').get_json()['kpi']['visits'],stats['kpi']['visits'])
        self.assertEqual(self.client.get('/visits/export.csv').status_code,200)
    def test_public_routes_and_source_isolation(self):
        for p in ['/','/privacy/','/assets/social.png','/robots.txt','/sitemap.xml']:
            self.assertEqual(self.client.get(p).status_code,200)
        for p in ['/wsgi.py','/dependencies.zip','/dashboard-password.txt','/api/stats','/visitor-badge.svg']:
            self.assertEqual(self.client.get(p).status_code,404)
    def test_private_dashboard_noindex(self):
        self.assertEqual(self.client.get('/visits').headers['X-Robots-Tag'],'noindex, nofollow')
    def test_geo_and_notifications_are_local_only(self):
        tracker=self.app.extensions['visitor_dashboard'].tracker
        self.assertFalse(tracker.geo.online)
        self.assertEqual(tracker.notify,'off')
        self.assertEqual(tracker.keep_days,180)

if __name__=='__main__':unittest.main()
