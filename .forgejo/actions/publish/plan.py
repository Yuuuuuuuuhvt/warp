import datetime,json,os,re,subprocess,urllib.request
from pathlib import Path

req=urllib.request.Request('https://git.luonto.me/api/v1/repos/yurun/warp/releases?limit=100',headers={'Authorization':'token '+os.environ['READ_TOKEN']})
with urllib.request.urlopen(req,timeout=60) as response:releases=json.load(response)
head=os.environ['HEAD'];stamp=int(os.environ['DATE'])
tag=next((r['tag_name'] for r in releases if r['tag_name'].endswith('-'+head[:9])),None)
release=next((r for r in releases if r['tag_name']==tag),None)
assets={a['name'] for a in release.get('assets',[])} if release else set()
force=os.environ.get('FORCE')=='true'
needs_mac=force or 'WarpOss.zip' not in assets or 'WarpOss.zip.sha256' not in assets
platforms=json.loads(Path('script/linux/oss/platforms.json').read_text())
matrix={'include':[]}
for name,p in platforms.items():
 if force or not any(a.endswith(p['suffix']) and a+'.sha256' in assets for a in assets):
  matrix['include'].append({'platform':name,'target':p['target'],'archive':p['archive']})
now=datetime.datetime.fromtimestamp(stamp,datetime.timezone.utc)
midnight=(now+datetime.timedelta(hours=8)).replace(hour=0,minute=0,second=0,microsecond=0)-datetime.timedelta(hours=8)
if not force:
 for r in releases:
  if r.get('draft') or r.get('prerelease'):continue
  names={a['name'] for a in r.get('assets',[])}
  complete='WarpOss.zip' in names and 'WarpOss.zip.sha256' in names and all(any(n.endswith(p['suffix']) and n+'.sha256' in names for n in names) for p in platforms.values())
  original=re.search(r'<!-- forgejo-migration-published-at: ([^ ]+) -->',r.get('body') or '')
  published=original.group(1) if original else r.get('published_at') or r.get('created_at')
  if complete and published and datetime.datetime.fromisoformat(published.replace('Z','+00:00'))>=midnight:
   needs_mac=False;matrix['include']=[];break
tag=tag or 'warposs-'+datetime.datetime.fromtimestamp(stamp,datetime.timezone.utc).strftime('%Y%m%d')+'-'+head[:9]
version=datetime.datetime.fromtimestamp(stamp,datetime.timezone.utc).strftime('%Y%m%d')+'.'+subprocess.check_output(['git','rev-list','--count','HEAD'],text=True).strip()
with open(os.environ['GITHUB_OUTPUT'],'a') as output:
 for k,v in {'tag':tag,'version':version,'need_macos':str(needs_mac).lower(),'need_linux':str(bool(matrix['include'])).lower(),'linux_matrix':json.dumps(matrix,separators=(',',':'))}.items():output.write(k+'='+v+'\n')
