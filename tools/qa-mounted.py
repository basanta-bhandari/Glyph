#!/usr/bin/env python3
"""Opt-in real FUSE/crash QA. Requires /dev/fuse; writes only --root and --work."""
import argparse,json,os,select,signal,subprocess,time
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--root',type=Path,required=True);p.add_argument('--work',type=Path,required=True);a=p.parse_args()
a.root.mkdir(parents=True,exist_ok=True);a.work.mkdir(parents=True,exist_ok=True)
base=[str(a.binary.resolve()),'--minimal','--root',str(a.root.resolve())]
def command(value):
 r=subprocess.run(base+['--command',value],text=True,capture_output=True,timeout=60)
 if r.returncode:raise RuntimeError(f'{value}: {r.stdout} {r.stderr}')
 return r.stdout.strip()
assert command('write /home/note safe')=='Written.'
assert command('cat /home/note')=='safe'
crash=subprocess.Popen(base+['--plain'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
crash.stdin.write('write /home/note unfinished\n');crash.stdin.flush()
assert select.select([crash.stdout],[],[],30)[0], 'session write timed out'
assert crash.stdout.readline().strip()=='Written.'
crash.kill();crash.wait(timeout=10)
assert command('cat /home/note')=='safe','did not recover last safe contents'
points=json.loads(command('snapshot list'));unfinished=next(s for s in points if s['label']=='unclean session (preserved)')
command(f"snapshot restore {unfinished['id']}")
assert command('cat /home/note')=='unfinished','unclean work was not preserved'
probe=a.work/'probe.py';probe.write_text('''import os
assert os.environ['HOME']=='/home'
assert not os.path.exists('/home/b454')
assert not os.path.exists('/run/user')
for p in ['/configs/init.lua','/AGENTS.md','/home/AGENTS.md']:
 try: open(p,'w'); raise AssertionError('writable policy '+p)
 except OSError: pass
open('/home/probe-ok','w').write('sandbox works')
print('confined mounted app passed')
''')
subprocess.run(base+['import',str(probe.resolve()),'probe'],check=True,timeout=60)
assert 'confined mounted app passed' in command('run probe')
assert command('cat /home/probe-ok')=='sandbox works'
assert not (a.root/'mountpoint.json').exists(),'mount marker was not removed'
print(json.dumps({'mounted_roundtrip':'passed','sigkill_last_safe':'passed','unclean_snapshot_restore':'passed','mounted_app_confinement':'passed','stale_mount_cleanup':'passed'}))
