import time
import unittest
from unittest.mock import patch, Mock
from ai.media_workflow import MediaWorkflow
from ai.tool_plan import parse_tool
from bob_service import Receiver


class MediaTests(unittest.TestCase):
    def test_cursor_target_requires_frame_and_ack_then_two_playing_frames(self):
        flow=MediaWorkflow('rick roll')
        launch=flow.next({'chrome':'closed'})
        self.assertIn('youtube.com/results',launch['args']['url'])
        flow.record(launch,{'ok':True})
        obs={'active_window':'Chrome','bridge_ready':True,'frame_id':'screen-a','targets':[
            {'kind':'video','label':'Rick Astley','id':'dQw4w9WgXcQ','x':.3,'y':.4}]}
        click=parse_tool(flow.next(obs))
        self.assertEqual(click['tool'],'click')
        self.assertEqual(click['args']['frame_id'],'screen-a')
        self.assertEqual(flow.stage,'choose')
        flow.record(click,{'ok':True})
        obs.update(media={'id':'dQw4w9WgXcQ','paused':False,'ended':False,'ad':False,'ready':4,'time':1.0},page_title='Rick Astley')
        with patch('ai.media_workflow.time.monotonic',return_value=10):
            self.assertEqual(flow.next(obs)['tool'],'observe')
        obs['media']['time']=3
        with patch('ai.media_workflow.time.monotonic',return_value=11):
            self.assertEqual(flow.next(obs)['tool'],'observe')  # same screenshot cannot verify playback
        obs['frame_id']='screen-b';obs['media']['time']=5
        with patch('ai.media_workflow.time.monotonic',return_value=12):
            self.assertEqual(flow.next(obs)['tool'],'done')
        obs['media']['paused']=True
        self.assertFalse(flow.verify(obs))

    def test_paused_player_requests_a_real_click_and_ads_never_complete(self):
        flow=MediaWorkflow('rick roll');flow.stage='verify';flow.selected='dQw4w9WgXcQ'
        obs={'active_window':'Chrome','bridge_ready':True,'frame_id':'frame','media':{
            'id':flow.selected,'paused':True,'ended':False,'ad':False,'ready':4,'time':10},
            'targets':[{'kind':'play','id':flow.selected,'label':'Play video','x':.2,'y':.8}]}
        self.assertEqual(flow.next(obs)['tool'],'click')
        obs['media'].update(paused=False,ad=True)
        self.assertEqual(flow.next(obs)['tool'],'observe')

    def test_receiver_requires_new_fresh_frame_for_completion(self):
        r=Receiver();r.task={'id':'task','mode':'do','text':'play video','planner':True};r.last_seen=time.monotonic()
        r.plan_step={'id':'step','plan':{'tool':'done','args':{'message':'done'}}}
        r.last_frame_id='old';r.last_frame_at=time.monotonic();r.plan_source_frame='old'
        r.planner.verify=Mock(return_value=True)
        def verify(frame):return r.handle(dict(type='planner_verify',protocol=1,nonce='test',task_id='task',step_id='step',frame_id=frame,observation={} ))[-1]
        self.assertEqual(verify('old')['type'],'error')
        r.planner.verify.assert_not_called()
        r.last_frame_id='new'
        self.assertEqual(verify('new')['verdict'],'verified')
        r.last_frame_at=time.monotonic()-6
        self.assertEqual(verify('new')['type'],'error')
        r.stop()
        self.assertEqual(verify('new')['type'],'error')

    def test_stale_click_permit_is_revoked(self):
        r=Receiver();r.task={'id':'task','mode':'do'};r.last_seen=time.monotonic();r.last_frame_id='new';r.last_frame_at=time.monotonic()
        r.plan_step={'id':'step','plan':{'tool':'click','args':{'frame_id':'old'}}}
        reply=r.handle(dict(type='planner_permit',protocol=1,nonce='test',task_id='task',step_id='step'))[-1]
        self.assertEqual(reply['type'],'error')
