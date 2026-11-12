#!/usr/bin/env python3
"""Generate technical diagrams for the human manual (Pillow, optional build-time tool)."""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont
out=Path(__file__).resolve().parents[1]/'assets/human/figures';out.mkdir(parents=True,exist_ok=True)
font='/usr/share/fonts/TTF/DejaVuSansMono.ttf'
if not Path(font).exists(): font='/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf'
def draw(name,title,boxes,footer):
 im=Image.new('RGB',(1200,520),'#181818');d=ImageDraw.Draw(im)
 f=ImageFont.truetype(font,23);small=ImageFont.truetype(font,18);large=ImageFont.truetype(font,36)
 d.text((46,33),title,fill='#e8d8b8',font=large)
 for i,(head,lines) in enumerate(boxes):
  x=46+i*380;d.rounded_rectangle((x,125,x+345,382),radius=15,fill='#242424',outline='#d6a35d',width=2)
  d.text((x+20,149),head,fill='#d6a35d',font=f)
  for j,line in enumerate(lines):d.text((x+20,203+j*36),line,fill='#e8d8b8',font=small)
  if i<2:d.line((x+350,251,x+374,251),fill='#d6a35d',width=3);d.polygon([(x+374,251),(x+365,245),(x+365,257)],fill='#d6a35d')
 d.text((46,431),footer,fill='#b7b1a7',font=small);im.save(out/f'{name}.png')
draw('workspace','GLYPH / a place for your tools',[
 ('YOU + LUA',['/configs','themes · links · apps','agent definitions','human controls']),
 ('THE WORKSPACE',['/home  your files','/apps  .app FILES','/system/source','games · CAD · browser']),
 ('THE BOUNDARY',['Bubblewrap namespace','children stay inside','declared host links','no host fallback'])],
 'One familiar terminal. Your tools, your choices, a boundary that actually exists.')
draw('storage','GLYPH / the disk underneath',[
 ('NORMAL FILE ACCESS',['editor · app · search','read only what you need','FUSE serves files','no full expanded mirror']),
 ('CHUNKS + TRANSACTIONS',['64 KiB compressed blocks','authenticated encryption','metadata + snapshots','writes commit as you go']),
 ('workspace.glyphdisk',['one SQLite disk image','last-safe recovery point','unclean work preserved','copy CLOSED for backup'])],
 'Compression under the floorboards. Snapshots help; a separate backup still matters.')
