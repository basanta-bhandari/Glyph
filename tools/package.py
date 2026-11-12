#!/usr/bin/env python3
"""Package a built Glyph runtime, optional Kat, and the original games. No host installation."""
import argparse, hashlib, json, os, shutil, subprocess, zipapp, tempfile
from pathlib import Path
parser=argparse.ArgumentParser()
parser.add_argument('--target',type=Path,default=Path(os.environ.get('CARGO_TARGET_DIR','target'))/'release')
parser.add_argument('--kat',type=Path)
a=parser.parse_args()
def copy_atomic(source,target):
 target.parent.mkdir(parents=True,exist_ok=True)
 with tempfile.NamedTemporaryFile(dir=target.parent,delete=False) as f: temporary=Path(f.name)
 try:
  shutil.copy2(source,temporary);temporary.replace(target)
 finally: temporary.unlink(missing_ok=True)
root=Path(__file__).resolve().parent.parent
dist=root/'dist'; (dist/'tools').mkdir(parents=True,exist_ok=True)
for name in ['glyph','glyph-bootstrap','glyph-browser','glyph-package-manager']:
 copy_atomic(a.target/name,dist/name)
for name in ['pytris','terminal-spiel']:
 zipapp.create_archive(root/'assets/games'/name,dist/'tools'/f'{name}.app',interpreter='/usr/bin/env python3',compressed=True)
if a.kat: copy_atomic(a.kat,dist/'tools/kat.app')
for name in ['LICENSE','THIRD_PARTY.md','CHANGELOG.md']:
 copy_atomic(root/name,dist/name)
copy_atomic(root/'assets/nvchad-starter/LICENSE',dist/'licenses/NvChad-starter.txt')
metadata=subprocess.run(['cargo','metadata','--locked','--format-version','1'],cwd=root,text=True,capture_output=True,check=True)
packages=json.loads(metadata.stdout)['packages']
(dist/'licenses/dependencies.json').write_text(json.dumps([{'name':p['name'],'version':p['version'],'license':p['license'],'repository':p.get('repository')} for p in packages],indent=2)+'\n')
for package in packages:
 directory=Path(package['manifest_path']).parent
 for item in directory.iterdir():
  if item.is_file() and item.name.upper().startswith(('LICENSE','COPYING','NOTICE')):
   copy_atomic(item,dist/'licenses'/f"{package['name']}-{package['version']}"/item.name)
files=sorted(p for p in dist.rglob('*') if p.is_file() and p.name!='SHA256SUMS')
(dist/'SHA256SUMS').write_text(''.join(f'{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.relative_to(dist)}\n' for p in files))
print(f'Packaged {len(files)} files in {dist}')
