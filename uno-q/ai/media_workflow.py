"""Bounded YouTube playback using fresh screen IDs and browser media evidence."""
import re
import time
import math
from urllib.parse import quote


def is_media(goal):
    return bool(re.search(r'\b(play|watch)\b',goal,re.I))


class MediaWorkflow:
    def __init__(self, query):
        self.query=query
        self.stage='launch'
        self.selected=''
        self.sample=None
        self.failed=False

    def playing(self,obs):
        media=obs.get('media') or {}
        frame=obs.get('frame_id')
        t=media.get('time')
        if (not obs.get('bridge_ready') or not frame or media.get('id')!=self.selected
                or media.get('paused') is not False or media.get('ended') is not False
                or media.get('ad') is not False or media.get('ready',0)<2
                or type(t) not in (int,float) or not math.isfinite(t) or t<0):
            self.sample=None
            return False
        old=self.sample
        now=time.monotonic()
        if old and old[0]!=frame and now-old[2]>=.5 and t>old[1]+.25:
            return True
        if old is None or now-old[2]>=.5:
            self.sample=(frame,t,now)
        return False

    def next(self,obs):
        if self.failed:
            return {'tool':'ask','args':{'question':'The video action failed. Please check the browser and try again.'}}
        if self.stage=='launch':
            return {'tool':'launch_chrome','args':{'url':'https://www.youtube.com/results?search_query='+quote(self.query)}}
        if obs.get('active_window')!='Chrome' or not obs.get('bridge_ready'):
            return {'tool':'observe','args':{}}
        targets=obs.get('targets') or []
        consent=next((x for x in targets if x.get('kind')=='consent'),None)
        target=consent
        if self.stage=='choose' and target is None:
            target=next((x for x in targets if x.get('kind')=='video'),None)
        elif self.stage=='verify' and target is None:
            if self.playing(obs):
                return {'tool':'done','args':{'message':'Verified video playback: '+str(obs.get('page_title','video'))[:180]}}
            media=obs.get('media') or {}
            if media.get('paused') is True and not media.get('ad'):
                target=next((x for x in targets if x.get('kind')=='play' and x.get('id')==self.selected),None)
        if target and obs.get('frame_id'):
            return {'tool':'click','args':{'x':target['x'],'y':target['y'],'frame_id':obs['frame_id'],
                                         'target_id':target.get('id') or 'consent','label':target['label']}}
        return {'tool':'observe','args':{}}

    def record(self,plan,result):
        if not result.get('ok'):
            if plan['tool']=='click' and result.get('error')=='Browser target changed; refreshing.':return
            if plan['tool']!='done':self.failed=True
            return
        if plan['tool']=='launch_chrome':self.stage='choose'
        elif plan['tool']=='click' and plan['args']['target_id']!='consent':
            self.selected=plan['args']['target_id']
            self.stage='verify'
            self.sample=None

    def verify(self,obs):
        return self.stage=='verify' and self.playing(obs)
