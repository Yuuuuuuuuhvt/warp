import json,os,urllib.error,urllib.parse,urllib.request
from pathlib import Path
from publish import Redirect

opener=urllib.request.build_opener(Redirect())
token=os.environ['RELEASE_WRITE_TOKEN']
url='https://git.luonto.me/api/v1/repos/yurun/warp/releases/tags/'+urllib.parse.quote(os.environ['RELEASE_TAG'],safe='')
try:
 with opener.open(urllib.request.Request(url,headers={'Authorization':'token '+token}),timeout=60) as response:release=json.load(response)
except urllib.error.HTTPError as error:
 if error.code!=404:raise
 release={'assets':[]}
output=Path('artifacts');output.mkdir(exist_ok=True)
for asset in release['assets']:
 name=asset['name']
 if Path(name).name!=name:raise RuntimeError('Invalid asset filename')
 target=output/name
 if target.exists():continue
 if urllib.parse.urlsplit(asset['browser_download_url']).hostname!='git.luonto.me':raise RuntimeError('Unexpected attachment origin')
 with opener.open(urllib.request.Request(asset['browser_download_url'],headers={'Authorization':'token '+token}),timeout=600) as response,target.open('wb') as f:
  for chunk in iter(lambda:response.read(1<<20),b''):f.write(chunk)
